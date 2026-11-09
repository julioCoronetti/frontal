use std::collections::HashMap;

use crate::protocol::StepState;
use crate::ui::highlight::IncrementalCodeHighlighter;
use crate::ui::markdown::MarkdownSegment;

pub mod format;
pub mod slice;

pub use format::wrap_text_utf8;

#[derive(Clone, Debug, PartialEq)]
pub struct TurnMeta {
    pub mode: String,
    pub model: String,
    pub duration_seconds: Option<f64>,
    pub output_tokens: Option<u64>,
}

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
    pub turn_meta: Option<TurnMeta>,
}

#[derive(Clone, Debug)]
pub struct CachedBlockLines {
    pub width: u16,
    pub revision: u64,
    pub lines: Vec<ratatui::text::Line<'static>>,
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

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn blocks(&self) -> &[MessageBlock] {
        &self.blocks
    }

    pub fn push_historical_assistant_message(&mut self, text: &str) -> u64 {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind: BlockKind::Assistant,
            content: text.to_string(),
            revision: 1,
            consolidated: true,
            collapsed: false,
            tool_args: None,
            step_index: None,
            code_highlighters: Vec::new(),
            markdown_ast: None,
            turn_meta: None,
        });
        id
    }

    pub fn push_historical_thought(&mut self, thought: &str) -> u64 {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind: BlockKind::Thought,
            content: thought.to_string(),
            revision: 1,
            consolidated: true,
            collapsed: true,
            tool_args: None,
            step_index: None,
            code_highlighters: Vec::new(),
            markdown_ast: None,
            turn_meta: None,
        });
        id
    }

    pub fn push_historical_tool_call(
        &mut self,
        step_index: Option<u64>,
        name: &str,
        args: Option<&str>,
        output: Option<&str>,
    ) -> u64 {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind: BlockKind::Tool {
                name: name.to_string(),
                state: StepState::Done,
            },
            content: output.unwrap_or_default().to_string(),
            revision: 1,
            consolidated: true,
            collapsed: true,
            tool_args: args.map(|s| s.to_string()),
            step_index,
            code_highlighters: Vec::new(),
            markdown_ast: None,
            turn_meta: None,
        });
        id
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
            turn_meta: None,
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
            turn_meta: None,
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
                    if state == StepState::Done {
                        block.collapsed = true;
                    }
                    block.revision += 1;
                    return block.id;
                }
            }
        }

        let id = self.next_block_id;
        self.next_block_id += 1;
        let is_collapsed = state == StepState::Done;
        self.blocks.push(MessageBlock {
            id,
            kind: BlockKind::Tool {
                name: name.to_string(),
                state,
            },
            content: output.unwrap_or("").to_string(),
            revision: 1,
            consolidated: false,
            collapsed: is_collapsed,
            tool_args: args.map(|s| s.to_string()),
            step_index,
            code_highlighters: Vec::new(),
            markdown_ast: None,
            turn_meta: None,
        });
        id
    }

    pub fn update_tool_state(&mut self, name: &str, state: StepState, output: Option<&str>) -> u64 {
        self.upsert_tool_call(None, name, state, None, output)
    }

    pub fn toggle_collapsible_at_line(&mut self, target_line: usize) -> Option<u64> {
        let mut current = 0;
        for block in &mut self.blocks {
            let count = self
                .l2_cache
                .get(&block.id)
                .map(|e| e.lines.len())
                .unwrap_or(0);
            if target_line >= current && target_line < current + count {
                if target_line == current
                    && matches!(block.kind, BlockKind::Tool { .. } | BlockKind::Thought)
                {
                    block.collapsed = !block.collapsed;
                    block.revision += 1;
                    return Some(block.id);
                }
                return None;
            }
            current += count;
        }
        None
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
            turn_meta: None,
        });
        id
    }

    pub fn consolidate_active_turn_with_meta(
        &mut self,
        fallback_response: Option<&str>,
        meta: Option<TurnMeta>,
    ) {
        let mut has_assistant_content = false;
        let mut last_assistant_idx = None;
        for (i, block) in self.blocks.iter_mut().enumerate() {
            if !block.consolidated {
                if matches!(block.kind, BlockKind::Assistant) && !block.content.trim().is_empty() {
                    has_assistant_content = true;
                    last_assistant_idx = Some(i);
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
                        turn_meta: meta.clone(),
                    });
                    return;
                }
            }
        }
        if let Some(idx) = last_assistant_idx {
            if let Some(block) = self.blocks.get_mut(idx) {
                block.turn_meta = meta;
                block.revision += 1;
            }
        }
    }

    pub fn consolidate_active_turn(&mut self, fallback_response: Option<&str>) {
        self.consolidate_active_turn_with_meta(fallback_response, None);
    }

    pub fn has_unconsolidated_blocks(&self) -> bool {
        self.blocks.iter().any(|b| !b.consolidated)
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
