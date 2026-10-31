use ropey::Rope;
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PasteId(pub u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExtmarkId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtmarkKind {
    FileMention { path: String },
    SlashCommand { cmd: String },
    PastePill { id: PasteId, line_count: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extmark {
    pub id: ExtmarkId,
    pub start_char: usize,
    pub end_char: usize,
    pub kind: ExtmarkKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    File,
    Command,
}

#[derive(Clone, Debug)]
pub struct EditorSnapshot {
    pub rope: Rope,
    pub cursor_char: usize,
    pub extmarks: Vec<Extmark>,
    pub paste_registry: HashMap<PasteId, String>,
    pub next_paste_id: u64,
    pub next_extmark_id: u64,
}

#[derive(Clone, Default)]
pub struct PromptEditor {
    rope: Rope,
    cursor_char: usize,
    extmarks: Vec<Extmark>,
    paste_registry: HashMap<PasteId, String>,
    next_paste_id: u64,
    next_extmark_id: u64,
}

impl PromptEditor {
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            cursor_char: 0,
            extmarks: Vec::new(),
            paste_registry: HashMap::new(),
            next_paste_id: 1,
            next_extmark_id: 1,
        }
    }

    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            rope: self.rope.clone(),
            cursor_char: self.cursor_char,
            extmarks: self.extmarks.clone(),
            paste_registry: self.paste_registry.clone(),
            next_paste_id: self.next_paste_id,
            next_extmark_id: self.next_extmark_id,
        }
    }

    pub fn restore_snapshot(&mut self, snap: EditorSnapshot) {
        self.rope = snap.rope;
        self.cursor_char = snap.cursor_char;
        self.extmarks = snap.extmarks;
        self.paste_registry = snap.paste_registry;
        self.next_paste_id = snap.next_paste_id;
        self.next_extmark_id = snap.next_extmark_id;
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    pub fn cursor_char(&self) -> usize {
        self.cursor_char
    }

    pub fn set_cursor_char(&mut self, pos: usize) {
        self.cursor_char = pos.min(self.rope.len_chars());
        self.adjust_cursor_outside_extmarks();
    }

    pub fn cursor_coordinates(&self) -> (usize, usize) {
        if self.rope.len_chars() == 0 {
            return (0, 0);
        }
        let safe_cursor = self.cursor_char.min(self.rope.len_chars());
        let line_idx = self.rope.char_to_line(safe_cursor);
        let line_start_char = self.rope.line_to_char(line_idx);
        let col_idx = safe_cursor.saturating_sub(line_start_char);
        (line_idx, col_idx)
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn extmarks(&self) -> &[Extmark] {
        &self.extmarks
    }

    pub fn insert_char(&mut self, ch: char) {
        self.rope.insert_char(self.cursor_char, ch);
        self.shift_extmarks_after(self.cursor_char, 1);
        self.cursor_char += 1;
    }

    pub fn insert_str(&mut self, text: &str) {
        let char_count = text.chars().count();
        self.rope.insert(self.cursor_char, text);
        self.shift_extmarks_after(self.cursor_char, char_count as isize);
        self.cursor_char += char_count;
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn insert_extmark(&mut self, kind: ExtmarkKind, display_text: &str) -> ExtmarkId {
        let start = self.cursor_char;
        let char_count = display_text.chars().count();
        self.rope.insert(start, display_text);
        self.shift_extmarks_after(start, char_count as isize);
        let end = start + char_count;
        let id = ExtmarkId(self.next_extmark_id);
        self.next_extmark_id += 1;
        self.extmarks.push(Extmark {
            id,
            start_char: start,
            end_char: end,
            kind,
        });
        self.cursor_char = end;
        id
    }

    pub fn insert_paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n");
        let line_count = normalized.lines().count();
        if line_count > 5 {
            let id = PasteId(self.next_paste_id);
            self.next_paste_id += 1;
            self.paste_registry.insert(id, normalized);
            let pill = format!("[Pasted #{}: ~{} lines]", id.0, line_count);
            self.insert_extmark(ExtmarkKind::PastePill { id, line_count }, &pill);
        } else {
            self.insert_str(&normalized);
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor_char == 0 {
            return;
        }

        let mut hit_idx = None;
        for (i, ext) in self.extmarks.iter().enumerate() {
            if self.cursor_char > ext.start_char && self.cursor_char <= ext.end_char {
                hit_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = hit_idx {
            let ext = self.extmarks.remove(idx);
            let len = ext.end_char - ext.start_char;
            self.rope.remove(ext.start_char..ext.end_char);
            if let ExtmarkKind::PastePill { id, .. } = ext.kind {
                self.paste_registry.remove(&id);
            }
            self.shift_extmarks_after(ext.start_char, -(len as isize));
            self.cursor_char = ext.start_char;
            return;
        }

        self.rope.remove((self.cursor_char - 1)..self.cursor_char);
        self.shift_extmarks_after(self.cursor_char - 1, -1);
        self.cursor_char -= 1;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor_char >= self.rope.len_chars() {
            return;
        }

        let mut hit_idx = None;
        for (i, ext) in self.extmarks.iter().enumerate() {
            if self.cursor_char >= ext.start_char && self.cursor_char < ext.end_char {
                hit_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = hit_idx {
            let ext = self.extmarks.remove(idx);
            let len = ext.end_char - ext.start_char;
            self.rope.remove(ext.start_char..ext.end_char);
            if let ExtmarkKind::PastePill { id, .. } = ext.kind {
                self.paste_registry.remove(&id);
            }
            self.shift_extmarks_after(ext.start_char, -(len as isize));
            self.cursor_char = ext.start_char;
            return;
        }

        self.rope.remove(self.cursor_char..(self.cursor_char + 1));
        self.shift_extmarks_after(self.cursor_char, -1);
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

    fn adjust_cursor_outside_extmarks(&mut self) {
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

    pub fn take_submitted_expanded(&mut self) -> Option<String> {
        let text = self.rope.to_string();
        if text.trim().is_empty() {
            self.clear();
            return None;
        }

        let mut cloned_rope = self.rope.clone();
        let mut sorted_extmarks: Vec<Extmark> = self
            .extmarks
            .iter()
            .filter(|e| matches!(e.kind, ExtmarkKind::PastePill { .. }))
            .cloned()
            .collect();
        sorted_extmarks.sort_by_key(|e| std::cmp::Reverse(e.start_char));

        for ext in sorted_extmarks {
            if let ExtmarkKind::PastePill { id, .. } = ext.kind {
                if let Some(payload) = self.paste_registry.get(&id) {
                    cloned_rope.remove(ext.start_char..ext.end_char);
                    cloned_rope.insert(ext.start_char, payload);
                }
            }
        }

        let final_str = cloned_rope.to_string();
        self.clear();
        Some(final_str)
    }

    pub fn clear(&mut self) {
        self.rope = Rope::new();
        self.cursor_char = 0;
        self.extmarks.clear();
        self.paste_registry.clear();
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

    pub fn replace_range_with_extmark(
        &mut self,
        start_char: usize,
        end_char: usize,
        kind: ExtmarkKind,
        display_text: &str,
    ) {
        let (token_text, has_trailing_space) =
            if let Some(stripped) = display_text.strip_suffix(' ') {
                (stripped, true)
            } else {
                (display_text, false)
            };

        let safe_end = end_char.min(self.rope.len_chars());
        let safe_start = start_char.min(safe_end);
        let removed_len = safe_end - safe_start;
        self.rope.remove(safe_start..safe_end);
        self.shift_extmarks_after(safe_start, -(removed_len as isize));

        let char_count = token_text.chars().count();
        self.rope.insert(safe_start, token_text);
        self.shift_extmarks_after(safe_start, char_count as isize);

        let id = ExtmarkId(self.next_extmark_id);
        self.next_extmark_id += 1;
        self.extmarks.push(Extmark {
            id,
            start_char: safe_start,
            end_char: safe_start + char_count,
            kind,
        });
        self.cursor_char = safe_start + char_count;

        if has_trailing_space {
            self.rope.insert_char(self.cursor_char, ' ');
            self.shift_extmarks_after(self.cursor_char, 1);
            self.cursor_char += 1;
        }
    }

    fn shift_extmarks_after(&mut self, threshold: usize, delta: isize) {
        for ext in &mut self.extmarks {
            if ext.start_char >= threshold {
                if delta >= 0 {
                    ext.start_char = ext.start_char.saturating_add(delta as usize);
                    ext.end_char = ext.end_char.saturating_add(delta as usize);
                } else {
                    let d = (-delta) as usize;
                    ext.start_char = ext.start_char.saturating_sub(d);
                    ext.end_char = ext.end_char.saturating_sub(d);
                }
            }
        }
    }
}
