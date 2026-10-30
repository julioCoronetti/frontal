use ratatui::text::Line;

use crate::ui::cache::RenderCache;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollMode {
    Anchored,
    Unanchored { offset: usize },
}

#[derive(Clone, Debug)]
pub struct ConversationViewport {
    mode: ScrollMode,
    total_lines: usize,
    viewport_height: usize,
}

impl ConversationViewport {
    pub fn new() -> Self {
        Self {
            mode: ScrollMode::Anchored,
            total_lines: 0,
            viewport_height: 20,
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

    pub fn visible_slice(&self, cache: &RenderCache) -> Vec<Line<'static>> {
        let offset = self.effective_offset();
        cache.visible_slice(offset, self.viewport_height)
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    pub fn viewport_height(&self) -> usize {
        self.viewport_height
    }
}

impl Default for ConversationViewport {
    fn default() -> Self {
        Self::new()
    }
}
