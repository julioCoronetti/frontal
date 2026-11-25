use std::path::{Path, PathBuf};

use crate::infra::fs_search::{self, FileSearchIndex};
use crate::ui::editor::TriggerKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutocompleteItem {
    pub label: String,
    pub insert_text: String,
    pub description: Option<String>,
    pub kind: TriggerKind,
}

#[derive(Clone, Debug)]
pub struct AutocompletePopover {
    pub visible: bool,
    pub kind: TriggerKind,
    pub trigger_start_char: usize,
    pub query: String,
    pub items: Vec<AutocompleteItem>,
    pub selected_idx: usize,
    pub query_seq: u64,
}

impl AutocompletePopover {
    pub fn new() -> Self {
        Self {
            visible: false,
            kind: TriggerKind::Command,
            trigger_start_char: 0,
            query: String::new(),
            items: Vec::new(),
            selected_idx: 0,
            query_seq: 0,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible && !self.items.is_empty()
    }

    pub fn show(
        &mut self,
        kind: TriggerKind,
        start_char: usize,
        query: &str,
        items: Vec<AutocompleteItem>,
    ) {
        self.visible = !items.is_empty();
        self.kind = kind;
        self.trigger_start_char = start_char;
        self.query = query.to_string();
        self.items = items;
        self.selected_idx = 0;
        self.query_seq = self.query_seq.wrapping_add(1);
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.items.clear();
        self.selected_idx = 0;
    }

    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected_idx = (self.selected_idx + 1) % self.items.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.items.is_empty() {
            if self.selected_idx == 0 {
                self.selected_idx = self.items.len() - 1;
            } else {
                self.selected_idx -= 1;
            }
        }
    }

    pub fn selected_item(&self) -> Option<&AutocompleteItem> {
        self.items.get(self.selected_idx)
    }

    pub fn selected_index(&self) -> usize {
        self.selected_idx
    }

    pub fn default_commands() -> Vec<AutocompleteItem> {
        let cmds = [
            ("/help", "Show available commands"),
            ("/clear", "Clear active conversation"),
            ("/new", "Open new session tab"),
            ("/model", "Select model and reasoning effort"),
            ("/mode", "Switch execution mode"),
            ("/resume", "Resume past session from history"),
            ("/rename", "Rename active session"),
            ("/quota", "Show model quotas and plan limits"),
            ("/quit", "Quit Frontal"),
        ];

        cmds.into_iter()
            .map(|(c, desc)| AutocompleteItem {
                label: c.to_string(),
                insert_text: format!("{} ", c),
                description: Some(desc.to_string()),
                kind: TriggerKind::Command,
            })
            .collect()
    }

    pub fn filter_commands(query: &str) -> Vec<AutocompleteItem> {
        let q = query.trim_start_matches('/').to_lowercase();
        let all = Self::default_commands();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|item| {
                item.label.to_lowercase().contains(&q)
                    || item
                        .description
                        .as_deref()
                        .map(|d| d.to_lowercase().contains(&q))
                        .unwrap_or(false)
            })
            .collect()
    }
}

impl Default for AutocompletePopover {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct FileSearchWorker {
    index: FileSearchIndex,
}

impl FileSearchWorker {
    pub fn new() -> Self {
        Self {
            index: FileSearchIndex::new(),
        }
    }

    pub fn set_cached_files(&mut self, files: Vec<String>) {
        self.index.set_files(files);
    }

    pub fn cached_files(&self) -> Vec<String> {
        self.index.snapshot().as_ref().clone()
    }

    pub fn start_background_scan(&self, root: PathBuf, max_entries: usize) {
        self.index.start_background_scan(root, max_entries);
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<AutocompleteItem> {
        let q = query.trim_start_matches('@').to_lowercase();
        let mut results = Vec::new();
        let files = self.index.snapshot();

        for file in files.iter() {
            if q.is_empty() || file.to_lowercase().contains(&q) {
                results.push(AutocompleteItem {
                    label: file.clone(),
                    insert_text: format!("@{} ", file),
                    description: None,
                    kind: TriggerKind::File,
                });
                if results.len() >= limit {
                    break;
                }
            }
        }

        results
    }

    pub fn scan_dir(root: &Path, max_entries: usize) -> Vec<String> {
        fs_search::scan_dir(root, max_entries)
    }
}

impl Default for FileSearchWorker {
    fn default() -> Self {
        Self::new()
    }
}
