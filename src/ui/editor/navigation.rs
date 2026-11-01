use super::{PromptEditor, TriggerKind};

impl PromptEditor {
    pub fn set_cursor_char(&mut self, pos: usize) {
        self.cursor_char = pos.min(self.rope.len_chars());
        self.adjust_cursor_outside_extmarks();
    }

    pub fn move_left(&mut self) {
        if self.cursor_char == 0 {
            return;
        }
        let mut new_cursor = self.cursor_char - 1;
        for ext in &self.extmarks {
            if ext.start_char < new_cursor && new_cursor < ext.end_char {
                new_cursor = ext.start_char;
                break;
            }
        }
        self.cursor_char = new_cursor;
    }

    pub fn move_right(&mut self) {
        if self.cursor_char >= self.rope.len_chars() {
            return;
        }
        let mut new_cursor = self.cursor_char + 1;
        for ext in &self.extmarks {
            if ext.start_char < new_cursor && new_cursor < ext.end_char {
                new_cursor = ext.end_char;
                break;
            }
        }
        self.cursor_char = new_cursor;
    }

    pub fn move_up(&mut self) -> bool {
        let (line_idx, col_idx) = self.cursor_coordinates();
        if line_idx == 0 {
            return false;
        }
        let prev_line = line_idx - 1;
        let prev_line_len = self.rope.line(prev_line).len_chars();
        let target_col = col_idx.min(prev_line_len.saturating_sub(1));
        let prev_line_start = self.rope.line_to_char(prev_line);
        self.cursor_char = prev_line_start + target_col;
        self.adjust_cursor_outside_extmarks();
        true
    }

    pub fn move_down(&mut self) -> bool {
        let (line_idx, col_idx) = self.cursor_coordinates();
        if line_idx + 1 >= self.rope.len_lines() {
            return false;
        }
        let next_line = line_idx + 1;
        let next_line_len = self.rope.line(next_line).len_chars();
        let target_col = col_idx.min(next_line_len.saturating_sub(1));
        let next_line_start = self.rope.line_to_char(next_line);
        self.cursor_char = next_line_start + target_col;
        self.adjust_cursor_outside_extmarks();
        true
    }

    pub fn move_home(&mut self) {
        let (line_idx, _) = self.cursor_coordinates();
        self.cursor_char = self.rope.line_to_char(line_idx);
        self.adjust_cursor_outside_extmarks();
    }

    pub fn move_end(&mut self) {
        let (line_idx, _) = self.cursor_coordinates();
        let line_chars = self.rope.line(line_idx).len_chars();
        let line_start = self.rope.line_to_char(line_idx);
        let is_last_line = line_idx + 1 == self.rope.len_lines();
        let offset = if is_last_line {
            line_chars
        } else {
            line_chars.saturating_sub(1)
        };
        self.cursor_char = line_start + offset;
        self.adjust_cursor_outside_extmarks();
    }

    pub(crate) fn adjust_cursor_outside_extmarks(&mut self) {
        for ext in &self.extmarks {
            if ext.start_char < self.cursor_char && self.cursor_char < ext.end_char {
                let dist_to_start = self.cursor_char - ext.start_char;
                let dist_to_end = ext.end_char - self.cursor_char;
                if dist_to_start <= dist_to_end {
                    self.cursor_char = ext.start_char;
                } else {
                    self.cursor_char = ext.end_char;
                }
                break;
            }
        }
    }

    pub fn active_trigger(&self) -> Option<(TriggerKind, usize, String)> {
        if self.cursor_char == 0 {
            return None;
        }

        for ext in &self.extmarks {
            if self.cursor_char > ext.start_char && self.cursor_char <= ext.end_char {
                return None;
            }
        }

        let mut start_idx = self.cursor_char;
        while start_idx > 0 {
            let ch = self.rope.char(start_idx - 1);
            if ch.is_whitespace() {
                break;
            }
            start_idx -= 1;
        }

        if start_idx >= self.cursor_char {
            return None;
        }

        let word_len = self.cursor_char - start_idx;
        let mut word = String::with_capacity(word_len);
        for i in start_idx..self.cursor_char {
            word.push(self.rope.char(i));
        }

        if let Some(stripped) = word.strip_prefix('@') {
            let is_boundary = start_idx == 0 || self.rope.char(start_idx - 1).is_whitespace();
            if is_boundary {
                return Some((TriggerKind::File, start_idx, stripped.to_string()));
            }
        } else if let Some(stripped) = word.strip_prefix('/') {
            let is_boundary = start_idx == 0;
            if is_boundary {
                return Some((TriggerKind::Command, start_idx, stripped.to_string()));
            }
        }

        None
    }
}
