use std::path::Path;

use crate::ui::editor::TriggerKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutocompleteItem {
    pub label: String,
    pub insert_text: String,
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
            ("/help", "Exibir ajuda de comandos"),
            ("/clear", "Limpar conversa ativa"),
            ("/compact", "Compactar historico de conversa"),
            ("/model", "Selecionar modelo"),
            ("/mode", "Alternar modo de operacao"),
            ("/interrupt", "Interromper execucao corrente"),
            ("/quit", "Sair do frontal"),
        ];

        cmds.into_iter()
            .map(|(c, desc)| AutocompleteItem {
                label: format!("{} - {}", c, desc),
                insert_text: format!("{} ", c),
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
            .filter(|item| item.insert_text.to_lowercase().contains(&q))
            .collect()
    }
}

impl Default for AutocompletePopover {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FileSearchWorker {
    cached_files: Vec<String>,
}

impl FileSearchWorker {
    pub fn new() -> Self {
        Self {
            cached_files: Vec::new(),
        }
    }

    pub fn set_cached_files(&mut self, files: Vec<String>) {
        self.cached_files = files;
    }

    pub fn cached_files(&self) -> &[String] {
        &self.cached_files
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<AutocompleteItem> {
        let q = query.trim_start_matches('@').to_lowercase();
        let mut results = Vec::new();

        for file in &self.cached_files {
            if q.is_empty() || file.to_lowercase().contains(&q) {
                results.push(AutocompleteItem {
                    label: file.clone(),
                    insert_text: format!("@{} ", file),
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
        let mut result = Vec::new();
        let mut stack = vec![root.to_path_buf()];

        while let Some(current) = stack.pop() {
            if result.len() >= max_entries {
                break;
            }

            let entries = match std::fs::read_dir(&current) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();

                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }

                if path.is_dir() {
                    stack.push(path);
                } else if path.is_file() {
                    if let Ok(rel) = path.strip_prefix(root) {
                        result.push(rel.to_string_lossy().to_string());
                    }
                    if result.len() >= max_entries {
                        break;
                    }
                }
            }
        }

        result.sort();
        result
    }
}

impl Default for FileSearchWorker {
    fn default() -> Self {
        Self::new()
    }
}
