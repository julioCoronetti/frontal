pub mod boundaries;
pub mod invariants;
pub mod lexer;

use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_str().unwrap();

        if path.is_dir() {
            if name_str != "target" && name_str != ".git" && name_str != ".wiki" {
                files.extend(collect_rs_files(&path));
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    files
}
