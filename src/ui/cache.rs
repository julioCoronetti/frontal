use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;

use crate::protocol::StepState;
use crate::ui::highlight::IncrementalCodeHighlighter;
use crate::ui::markdown::{
    parse_markdown_segments, render_markdown_to_lines, render_segments_to_lines, MarkdownSegment,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    User,
    Assistant,
    Thought,
    Tool { name: String, state: StepState },
    Error,
}

#[derive(Clone)]
pub struct MessageBlock {
    pub id: u64,
    pub kind: BlockKind,
    pub content: String,
    pub revision: u64,
    pub consolidated: bool,
    pub collapsed: bool,
    pub tool_args: Option<String>,
    pub step_index: Option<u64>,
    pub code_highlighters: Vec<IncrementalCodeHighlighter>,
    pub markdown_ast: Option<Vec<MarkdownSegment>>,
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
            collapsed: false,
            tool_args: None,
            step_index: None,
            code_highlighters: Vec::new(),
            markdown_ast: None,
        });
        id
    }

    pub fn append_streaming_delta(&mut self, kind: BlockKind, delta: &str) -> u64 {
        for block in self.blocks.iter_mut().rev() {
            if !block.consolidated && block.kind == kind {
                block.content.push_str(delta);
                block.revision += 1;
                block.markdown_ast = None;
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
            collapsed: false,
            tool_args: None,
            step_index: None,
            code_highlighters: Vec::new(),
            markdown_ast: None,
        });
        id
    }

    pub fn upsert_tool_call(
        &mut self,
        step_index: Option<u64>,
        name: &str,
        state: StepState,
        args: Option<&str>,
        output: Option<&str>,
    ) -> u64 {
        for block in self.blocks.iter_mut().rev() {
            if !block.consolidated {
                let matches = if let Some(idx) = step_index {
                    block.step_index == Some(idx)
                } else if let BlockKind::Tool {
                    name: ref n,
                    state: ref s,
                } = block.kind
                {
                    n == name && *s == StepState::Active
                } else {
                    false
                };

                if matches {
                    block.kind = BlockKind::Tool {
                        name: name.to_string(),
                        state,
                    };
                    if let Some(a) = args {
                        block.tool_args = Some(a.to_string());
                    }
                    if let Some(out) = output {
                        block.content = out.to_string();
                    }
                    block.revision += 1;
                    return block.id;
                }
            }
        }

        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind: BlockKind::Tool {
                name: name.to_string(),
                state,
            },
            content: output.unwrap_or("").to_string(),
            revision: 1,
            consolidated: false,
            collapsed: false,
            tool_args: args.map(|s| s.to_string()),
            step_index,
            code_highlighters: Vec::new(),
            markdown_ast: None,
        });
        id
    }

    pub fn update_tool_state(&mut self, name: &str, state: StepState, output: Option<&str>) -> u64 {
        self.upsert_tool_call(None, name, state, None, output)
    }

    pub fn toggle_block_collapsed(&mut self, block_id: u64) -> bool {
        for block in &mut self.blocks {
            if block.id == block_id {
                block.collapsed = !block.collapsed;
                block.revision += 1;
                return block.collapsed;
            }
        }
        false
    }

    pub fn toggle_latest_thought(&mut self) -> Option<u64> {
        for block in self.blocks.iter_mut().rev() {
            if matches!(block.kind, BlockKind::Thought) {
                block.collapsed = !block.collapsed;
                block.revision += 1;
                return Some(block.id);
            }
        }
        None
    }

    pub fn toggle_latest_tool(&mut self) -> Option<u64> {
        for block in self.blocks.iter_mut().rev() {
            if matches!(block.kind, BlockKind::Tool { .. }) {
                block.collapsed = !block.collapsed;
                block.revision += 1;
                return Some(block.id);
            }
        }
        None
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
            collapsed: false,
            tool_args: None,
            step_index: None,
            code_highlighters: Vec::new(),
            markdown_ast: None,
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
                        collapsed: false,
                        tool_args: None,
                        step_index: None,
                        code_highlighters: Vec::new(),
                        markdown_ast: None,
                    });
                }
            }
        }
    }

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

    fn format_block_lines(block: &mut MessageBlock, width: u16) -> Vec<Line<'static>> {
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
                let lines = if block.consolidated {
                    if block.markdown_ast.is_none() {
                        block.markdown_ast = Some(parse_markdown_segments(&block.content, true));
                    }
                    render_segments_to_lines(
                        block.markdown_ast.as_ref().unwrap(),
                        width as usize,
                        &mut block.code_highlighters,
                    )
                } else {
                    render_markdown_to_lines(
                        &block.content,
                        width as usize,
                        false,
                        &mut block.code_highlighters,
                    )
                };
                result.extend(lines);
                if block.consolidated {
                    result.push(Line::from(""));
                }
            }
            BlockKind::Thought => {
                if block.collapsed {
                    let line_count = block.content.lines().count().max(1);
                    result.push(Line::from(vec![Span::styled(
                        format!("▸ Thought [collapsed - {} lines]", line_count),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )]));
                } else {
                    result.push(Line::from(vec![Span::styled(
                        "▾ Thought",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )]));
                    let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                    for row in wrapped {
                        result.push(Line::from(vec![
                            Span::styled("  ▎ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                row,
                                Style::default()
                                    .fg(Color::DarkGray)
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
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    ),
                };

                let args_str = block
                    .tool_args
                    .as_deref()
                    .map(|a| format!("({})", a))
                    .unwrap_or_default();

                if block.collapsed {
                    let header = format!("{} {}{} ▸ [collapsed]", state_badge, name, args_str);
                    result.push(Line::from(vec![Span::styled(header, state_style)]));
                } else {
                    let header = format!("{} {}{}", state_badge, name, args_str);
                    result.push(Line::from(vec![Span::styled(header, state_style)]));

                    if !block.content.is_empty() {
                        let wrapped =
                            wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                        for row in wrapped {
                            let text_style = if matches!(state, StepState::Error) {
                                Style::default().fg(Color::LightRed)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            };
                            result.push(Line::from(vec![
                                Span::styled("  ", Style::default()),
                                Span::styled(row, text_style),
                            ]));
                        }
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

    pub fn clear(&mut self) {
        self.blocks.clear();
        self.l2_cache.clear();
    }
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::new()
    }
}
