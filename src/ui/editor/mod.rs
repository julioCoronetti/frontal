use ropey::Rope;
use std::collections::HashMap;

mod mutations;
mod navigation;

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
    pub(crate) rope: Rope,
    pub(crate) cursor_char: usize,
    pub(crate) extmarks: Vec<Extmark>,
    pub(crate) paste_registry: HashMap<PasteId, String>,
    pub(crate) next_paste_id: u64,
    pub(crate) next_extmark_id: u64,
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
}

pub fn sanitize_paste_text(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some(&'[') = chars.peek() {
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            } else if let Some(&']') = chars.peek() {
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\x07' || c == '\x1b' {
                        break;
                    }
                }
            }
            continue;
        }
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            clean.push('\n');
        } else if ch == '\n' || ch == '\t' || !ch.is_control() {
            clean.push(ch);
        }
    }
    clean
}
