use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_rs_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files
}

fn strip_string_literals(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_str = false;
    let mut escape = false;

    for ch in line.chars() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
        } else {
            out.push(ch);
        }
    }
    out
}

#[test]
fn test_ceiling_500_lines_per_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut targets = collect_rs_files(&root.join("src"));
    targets.extend(collect_rs_files(&root.join("tests")));

    let mut violations = Vec::new();
    for file in targets {
        let content = fs::read_to_string(&file).unwrap();
        let line_count = content.lines().count();
        if line_count > 500 {
            violations.push(format!("{}: {} lines", file.display(), line_count));
        }
    }

    assert!(
        violations.is_empty(),
        "500-line ceiling violated in the following files:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_zero_comments_in_codebase() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut targets = collect_rs_files(&root.join("src"));
    targets.extend(collect_rs_files(&root.join("tests")));

    let mut violations = Vec::new();
    for file in targets {
        let content = fs::read_to_string(&file).unwrap();
        for (idx, line) in content.lines().enumerate() {
            let stripped = strip_string_literals(line);
            if stripped.contains("//") || stripped.contains("/*") || stripped.contains("#[doc") {
                violations.push(format!("{}:{}: {}", file.display(), idx + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Zero-comment rule violated in the following files:\n{}",
        violations.join("\n")
    );
}
