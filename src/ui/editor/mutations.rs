use super::{
    sanitize_paste_text, EditGroup, Extmark, ExtmarkId, ExtmarkKind, PasteId, PromptEditor,
};

impl PromptEditor {
    pub(crate) fn delete_char_range(&mut self, start: usize, end: usize) {
        if start >= end || self.rope.len_chars() == 0 {
            return;
        }
        let mut exp_start = start.min(self.rope.len_chars());
        let mut exp_end = end.min(self.rope.len_chars());
        if exp_start >= exp_end {
            return;
        }

        let mut changed = true;
        while changed {
            changed = false;
            for ext in &self.extmarks {
                if ext.start_char < exp_end && ext.end_char > exp_start {
                    if ext.start_char < exp_start {
                        exp_start = ext.start_char;
                        changed = true;
                    }
                    if ext.end_char > exp_end {
                        exp_end = ext.end_char;
                        changed = true;
                    }
                }
            }
        }

        let mut removed_pill_ids = Vec::new();
        self.extmarks.retain(|ext| {
            let inside = ext.start_char >= exp_start && ext.end_char <= exp_end;
            if inside {
                if let ExtmarkKind::PastePill { id, .. } = ext.kind {
                    removed_pill_ids.push(id);
                }
            }
            !inside
        });
        for id in removed_pill_ids {
            self.paste_registry.remove(&id);
        }

        self.rope.remove(exp_start..exp_end);
        let removed_len = exp_end - exp_start;
        self.shift_extmarks_after(exp_start, -(removed_len as isize));
        self.cursor_char = exp_start;
        self.selection = None;
    }

    pub fn delete_to_line_start(&mut self) -> bool {
        let (line_idx, _) = self.cursor_coordinates();
        let line_start = self.rope.line_to_char(line_idx);
        if self.cursor_char > line_start {
            self.record_undo_snapshot();
            self.delete_char_range(line_start, self.cursor_char);
            self.last_edit_group = EditGroup::None;
            true
        } else {
            false
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        let had_selection = if let Some((s, e)) = self.selection {
            self.record_undo_snapshot();
            self.delete_char_range(s, e);
            true
        } else {
            false
        };
        let current_group = if ch.is_whitespace() {
            EditGroup::Whitespace
        } else {
            EditGroup::Word
        };
        if !had_selection
            && (self.last_edit_group != current_group || self.last_edit_group == EditGroup::None)
        {
            self.record_undo_snapshot();
        }
        self.last_edit_group = current_group;
        self.rope.insert_char(self.cursor_char, ch);
        self.shift_extmarks_after(self.cursor_char, 1);
        self.cursor_char += 1;
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.record_undo_snapshot();
        if let Some((s, e)) = self.selection {
            self.delete_char_range(s, e);
        }
        self.last_edit_group = EditGroup::None;
        let char_count = text.chars().count();
        self.rope.insert(self.cursor_char, text);
        self.shift_extmarks_after(self.cursor_char, char_count as isize);
        self.cursor_char += char_count;
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn insert_extmark(&mut self, kind: ExtmarkKind, display_text: &str) -> ExtmarkId {
        self.record_undo_snapshot();
        if let Some((s, e)) = self.selection {
            self.delete_char_range(s, e);
        }
        self.last_edit_group = EditGroup::None;
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
        self.record_undo_snapshot();
        if let Some((s, e)) = self.selection {
            self.delete_char_range(s, e);
        }
        self.last_edit_group = EditGroup::None;
        let normalized = sanitize_paste_text(text);
        let line_count = normalized.lines().count();
        if line_count > 5 {
            let id = PasteId(self.next_paste_id);
            self.next_paste_id += 1;
            self.paste_registry.insert(id, normalized);
            let pill = format!("[Pasted #{}: ~{} lines]", id.0, line_count);
            let start = self.cursor_char;
            let char_count = pill.chars().count();
            self.rope.insert(start, &pill);
            self.shift_extmarks_after(start, char_count as isize);
            let end = start + char_count;
            let id_ext = ExtmarkId(self.next_extmark_id);
            self.next_extmark_id += 1;
            self.extmarks.push(Extmark {
                id: id_ext,
                start_char: start,
                end_char: end,
                kind: ExtmarkKind::PastePill { id, line_count },
            });
            self.cursor_char = end;
        } else {
            let char_count = normalized.chars().count();
            self.rope.insert(self.cursor_char, &normalized);
            self.shift_extmarks_after(self.cursor_char, char_count as isize);
            self.cursor_char += char_count;
        }
    }

    pub fn backspace(&mut self) {
        if let Some((s, e)) = self.selection {
            self.record_undo_snapshot();
            self.delete_char_range(s, e);
            self.last_edit_group = EditGroup::None;
            return;
        }

        if self.cursor_char == 0 {
            return;
        }

        self.record_undo_snapshot();
        self.last_edit_group = EditGroup::None;

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
        if let Some((s, e)) = self.selection {
            self.record_undo_snapshot();
            self.delete_char_range(s, e);
            self.last_edit_group = EditGroup::None;
            return;
        }

        if self.cursor_char >= self.rope.len_chars() {
            return;
        }

        self.record_undo_snapshot();
        self.last_edit_group = EditGroup::None;

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

    pub fn replace_range_with_extmark(
        &mut self,
        start_char: usize,
        end_char: usize,
        kind: ExtmarkKind,
        display_text: &str,
    ) {
        self.record_undo_snapshot();
        self.last_edit_group = EditGroup::None;

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

    pub(crate) fn shift_extmarks_after(&mut self, threshold: usize, delta: isize) {
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
