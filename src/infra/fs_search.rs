use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tokio::sync::watch;

#[derive(Clone)]
pub struct FileSearchIndex {
    tx: Arc<watch::Sender<Arc<Vec<String>>>>,
    rx: watch::Receiver<Arc<Vec<String>>>,
}

impl FileSearchIndex {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(Arc::new(Vec::new()));
        Self {
            tx: Arc::new(tx),
            rx,
        }
    }

    pub fn set_files(&mut self, files: Vec<String>) {
        let _ = self.tx.send(Arc::new(files));
    }

    pub fn snapshot(&self) -> Arc<Vec<String>> {
        self.rx.borrow().clone()
    }

    pub fn start_background_scan(&self, root: PathBuf, max_entries: usize) {
        let tx = Arc::clone(&self.tx);
        thread::spawn(move || {
            let files = scan_dir(&root, max_entries);
            let _ = tx.send(Arc::new(files));
        });
    }
}

impl Default for FileSearchIndex {
    fn default() -> Self {
        Self::new()
    }
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
