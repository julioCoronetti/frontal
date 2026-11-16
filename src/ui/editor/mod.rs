use ropey::Rope;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use crate::protocol::{MediaItem, PromptPayload};

mod mutations;
mod navigation;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditGroup {
    #[default]
    None,
    Word,
    Whitespace,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PasteId(pub u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExtmarkId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtmarkKind {
    FileMention { path: String },
    SlashCommand { cmd: String },
    PastePill { id: PasteId, line_count: usize },
    Media { mime_type: String, uri: String },
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
    pub next_media_id: u64,
    pub selection: Option<(usize, usize)>,
}

#[derive(Clone, Default)]
pub struct PromptEditor {
    pub(crate) rope: Rope,
    pub(crate) cursor_char: usize,
    pub(crate) extmarks: Vec<Extmark>,
    pub(crate) paste_registry: HashMap<PasteId, String>,
    pub(crate) next_paste_id: u64,
    pub(crate) next_extmark_id: u64,
    pub(crate) next_media_id: u64,
    pub(crate) undo_stack: VecDeque<EditorSnapshot>,
    pub(crate) selection: Option<(usize, usize)>,
    pub(crate) last_edit_group: EditGroup,
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
            next_media_id: 1,
            undo_stack: VecDeque::new(),
            selection: None,
            last_edit_group: EditGroup::None,
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
            next_media_id: self.next_media_id,
            selection: self.selection,
        }
    }

    pub fn restore_snapshot(&mut self, snap: EditorSnapshot) {
        self.rope = snap.rope;
        self.cursor_char = snap.cursor_char;
        self.extmarks = snap.extmarks;
        self.paste_registry = snap.paste_registry;
        self.next_paste_id = snap.next_paste_id;
        self.next_extmark_id = snap.next_extmark_id;
        self.next_media_id = snap.next_media_id;
        self.selection = snap.selection;
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop_back() {
            self.restore_snapshot(prev);
            self.last_edit_group = EditGroup::None;
            true
        } else {
            false
        }
    }

    pub(crate) fn record_undo_snapshot(&mut self) {
        if self.undo_stack.len() >= 100 {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(self.snapshot());
    }

    pub fn select_all(&mut self) {
        if self.rope.len_chars() > 0 {
            self.selection = Some((0, self.rope.len_chars()));
            self.cursor_char = self.rope.len_chars();
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection.map(|(a, b)| (a.min(b), a.max(b)))
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    pub fn selected_text(&self) -> Option<String> {
        let (s, e) = self.selection?;
        let (start, end) = (s.min(e), s.max(e));
        if start >= end || start >= self.rope.len_chars() {
            return None;
        }
        let safe_end = end.min(self.rope.len_chars());
        let mut cloned = self.rope.slice(start..safe_end).to_string();

        let mut sorted_pills: Vec<Extmark> = self
            .extmarks
            .iter()
            .filter(|e| {
                matches!(e.kind, ExtmarkKind::PastePill { .. })
                    && e.start_char >= start
                    && e.end_char <= safe_end
            })
            .cloned()
            .collect();
        sorted_pills.sort_by_key(|e| std::cmp::Reverse(e.start_char));

        for ext in sorted_pills {
            if let ExtmarkKind::PastePill { id, .. } = ext.kind {
                if let Some(payload) = self.paste_registry.get(&id) {
                    let rel_start = ext.start_char - start;
                    let rel_end = ext.end_char - start;
                    let pill_str = format!(
                        "[Pasted #{}: ~{} lines]",
                        id.0,
                        match ext.kind {
                            ExtmarkKind::PastePill { line_count, .. } => line_count,
                            _ => 0,
                        }
                    );
                    if cloned.contains(&pill_str) {
                        cloned = cloned.replacen(&pill_str, payload, 1);
                    } else if rel_start <= cloned.len() && rel_end <= cloned.len() {
                        cloned.replace_range(rel_start..rel_end, payload);
                    }
                }
            }
        }

        if cloned.is_empty() {
            None
        } else {
            Some(cloned)
        }
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
        self.take_submitted_payload().map(|p| p.summary_text())
    }

    pub fn take_submitted_payload(&mut self) -> Option<PromptPayload> {
        let mut media_exts: Vec<Extmark> = self
            .extmarks
            .iter()
            .filter(|e| matches!(e.kind, ExtmarkKind::Media { .. }))
            .cloned()
            .collect();
        media_exts.sort_by_key(|e| e.start_char);

        let media: Vec<MediaItem> = media_exts
            .iter()
            .filter_map(|e| match &e.kind {
                ExtmarkKind::Media { mime_type, uri } => Some(MediaItem {
                    mime_type: mime_type.clone(),
                    uri: uri.clone(),
                }),
                _ => None,
            })
            .collect();

        let text = self.rope.to_string();
        if text.trim().is_empty() && media.is_empty() {
            self.clear();
            return None;
        }

        let mut cloned_rope = self.rope.clone();
        let mut sorted_extmarks: Vec<Extmark> = self
            .extmarks
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    ExtmarkKind::PastePill { .. } | ExtmarkKind::Media { .. }
                )
            })
            .cloned()
            .collect();
        sorted_extmarks.sort_by_key(|e| std::cmp::Reverse(e.start_char));

        for ext in sorted_extmarks {
            match ext.kind {
                ExtmarkKind::PastePill { id, .. } => {
                    if let Some(payload) = self.paste_registry.get(&id) {
                        cloned_rope.remove(ext.start_char..ext.end_char);
                        cloned_rope.insert(ext.start_char, payload);
                    }
                }
                ExtmarkKind::Media { .. } => {
                    cloned_rope.remove(ext.start_char..ext.end_char);
                }
                _ => {}
            }
        }

        let raw_text = cloned_rope.to_string();
        let final_text = if media.is_empty() {
            raw_text
        } else {
            raw_text.trim().to_string()
        };
        if final_text.trim().is_empty() && media.is_empty() {
            self.clear();
            return None;
        }

        self.clear();
        self.undo_stack.clear();
        Some(PromptPayload::new(final_text, media))
    }

    pub fn clear(&mut self) {
        self.rope = Rope::new();
        self.cursor_char = 0;
        self.extmarks.clear();
        self.paste_registry.clear();
        self.selection = None;
        self.last_edit_group = EditGroup::None;
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

pub fn image_mime_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn unescape_shell_path(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if matches!(next, ' ' | '\'' | '"' | '\\' | '(' | ')' | '[' | ']') {
                    chars.next();
                    out.push(next);
                    continue;
                }
            }
        }
        out.push(ch);
    }
    out
}

pub fn extract_pasted_image_path(raw: &str) -> Option<(PathBuf, &'static str)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
        return None;
    }
    let unquoted = if (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
        || (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let normalized = if let Some(rest) = unquoted.strip_prefix("file://localhost") {
        percent_decode_utf8(rest)
    } else if let Some(rest) = unquoted.strip_prefix("file://") {
        percent_decode_utf8(rest)
    } else {
        unescape_shell_path(unquoted)
    };
    let path = PathBuf::from(normalized);
    let mime = image_mime_for_path(&path)?;
    if path.is_file() {
        Some((path, mime))
    } else {
        None
    }
}
