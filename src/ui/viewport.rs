use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::ui::cache::RenderCache;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollMode {
    Anchored,
    Unanchored { offset: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionPoint {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
pub struct ConversationViewport {
    mode: ScrollMode,
    total_lines: usize,
    viewport_height: usize,
    pub selection: Option<(SelectionPoint, SelectionPoint)>,
}

impl ConversationViewport {
    pub fn new() -> Self {
        Self {
            mode: ScrollMode::Anchored,
            total_lines: 0,
            viewport_height: 20,
            selection: None,
        }
    }

    pub fn mode(&self) -> ScrollMode {
        self.mode
    }

    pub fn is_anchored(&self) -> bool {
        matches!(self.mode, ScrollMode::Anchored)
    }

    pub fn effective_offset(&self) -> usize {
        let max_scroll = self.total_lines.saturating_sub(self.viewport_height);
        match self.mode {
            ScrollMode::Anchored => max_scroll,
            ScrollMode::Unanchored { offset } => offset.min(max_scroll),
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        if self.total_lines <= self.viewport_height {
            self.mode = ScrollMode::Anchored;
            return;
        }
        let current = self.effective_offset();
        let new_offset = current.saturating_sub(lines);
        self.mode = ScrollMode::Unanchored { offset: new_offset };
    }

    pub fn scroll_down(&mut self, lines: usize) {
        let max_scroll = self.total_lines.saturating_sub(self.viewport_height);
        let current = self.effective_offset();
        let new_offset = current.saturating_add(lines);
        if new_offset >= max_scroll {
            self.mode = ScrollMode::Anchored;
        } else {
            self.mode = ScrollMode::Unanchored { offset: new_offset };
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.mode = ScrollMode::Anchored;
    }

    pub fn sync_dimensions(&mut self, total_lines: usize, viewport_height: usize) {
        self.total_lines = total_lines;
        self.viewport_height = viewport_height.max(1);
        let max_scroll = self.total_lines.saturating_sub(self.viewport_height);
        if let ScrollMode::Unanchored { offset } = self.mode {
            if offset >= max_scroll {
                self.mode = ScrollMode::Anchored;
            }
        }
    }

    pub fn start_selection(&mut self, line: usize, col: usize) {
        let p = SelectionPoint { line, col };
        self.selection = Some((p, p));
    }

    pub fn update_selection(&mut self, line: usize, col: usize) {
        if let Some((start, _)) = self.selection {
            self.selection = Some((start, SelectionPoint { line, col }));
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn selected_text(&self, cache: &RenderCache) -> Option<String> {
        let (start, end) = self.selection?;
        if start == end {
            return None;
        }
        let text = cache.extract_text_range(start.line, start.col, end.line, end.col);
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    }

    pub fn visible_slice(&self, cache: &RenderCache) -> Vec<Line<'static>> {
        let offset = self.effective_offset();
        let mut lines = cache.visible_slice(offset, self.viewport_height);
        if let Some((start, end)) = self.selection {
            if start == end {
                return lines;
            }

            let (s_line, s_col, e_line, e_col) =
                if start.line > end.line || (start.line == end.line && start.col > end.col) {
                    (end.line, end.col, start.line, start.col)
                } else {
                    (start.line, start.col, end.line, end.col)
                };

            for (i, line) in lines.iter_mut().enumerate() {
                let abs_line = offset + i;
                if abs_line < s_line || abs_line > e_line {
                    continue;
                }

                if abs_line == s_line && abs_line == e_line {
                    highlight_line_range(line, s_col, e_col);
                } else if abs_line == s_line {
                    highlight_line_range(line, s_col, usize::MAX);
                } else if abs_line == e_line {
                    highlight_line_range(line, 0, e_col);
                } else {
                    for span in &mut line.spans {
                        span.style = span.style.add_modifier(Modifier::REVERSED);
                    }
                }
            }
        }
        lines
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    pub fn viewport_height(&self) -> usize {
        self.viewport_height
    }
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn highlight_line_range(line: &mut Line<'static>, from_col: usize, to_col: usize) {
    if from_col >= to_col {
        return;
    }
    let mut new_spans = Vec::new();
    let mut current_col = 0;

    for mut span in std::mem::take(&mut line.spans) {
        let span_len = span.content.chars().count();
        let span_end = current_col + span_len;

        if span_end <= from_col || current_col >= to_col {
            new_spans.push(span);
        } else if current_col >= from_col && span_end <= to_col {
            span.style = span.style.add_modifier(Modifier::REVERSED);
            new_spans.push(span);
        } else {
            let rel_start = from_col.saturating_sub(current_col).min(span_len);
            let rel_end = to_col.saturating_sub(current_col).min(span_len);

            let content = span.content.as_ref();
            let b_start = char_to_byte_idx(content, rel_start);
            let b_end = char_to_byte_idx(content, rel_end);

            if b_start > 0 {
                new_spans.push(Span::styled(content[..b_start].to_string(), span.style));
            }
            if b_start < b_end {
                new_spans.push(Span::styled(
                    content[b_start..b_end].to_string(),
                    span.style.add_modifier(Modifier::REVERSED),
                ));
            }
            if b_end < content.len() {
                new_spans.push(Span::styled(content[b_end..].to_string(), span.style));
            }
        }
        current_col = span_end;
    }
    line.spans = new_spans;
}

impl Default for ConversationViewport {
    fn default() -> Self {
        Self::new()
    }
}
