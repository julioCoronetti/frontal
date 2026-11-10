use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
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

pub fn scan_rust_source_violations(content: &str) -> Vec<(usize, String)> {
    let mut violations = Vec::new();
    let chars: Vec<char> = content.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut line = 1;
    let mut prev_char = ' ';

    while i < n {
        let ch = chars[i];
        if ch == '\n' {
            line += 1;
            i += 1;
            prev_char = '\n';
            continue;
        }

        if ch == '\'' {
            if i + 2 < n && chars[i + 1] != '\\' && chars[i + 2] == '\'' {
                i += 3;
                prev_char = '\'';
                continue;
            } else if i + 3 < n && chars[i + 1] == '\\' && chars[i + 3] == '\'' {
                i += 4;
                prev_char = '\'';
                continue;
            } else if i + 1 < n && chars[i + 1] == '\\' {
                let mut end = i + 2;
                while end < n && end - i <= 10 && chars[end] != '\n' && chars[end] != '\'' {
                    end += 1;
                }
                if end < n && chars[end] == '\'' {
                    i = end + 1;
                    prev_char = '\'';
                    continue;
                }
            }
        }

        let is_ident = prev_char.is_alphanumeric() || prev_char == '_';
        let is_raw_lead =
            ch == 'r' || ((ch == 'b' || ch == 'c') && i + 1 < n && chars[i + 1] == 'r');
        if is_raw_lead && !is_ident {
            let r_idx = if ch == 'r' { i } else { i + 1 };
            let mut h = r_idx + 1;
            while h < n && chars[h] == '#' {
                h += 1;
            }
            if h < n && chars[h] == '"' {
                let hashes = h - (r_idx + 1);
                i = h + 1;
                let mut closing = Vec::with_capacity(1 + hashes);
                closing.push('"');
                closing.resize(1 + hashes, '#');
                while i < n {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    if chars[i..].starts_with(&closing) {
                        i += closing.len();
                        break;
                    }
                    i += 1;
                }
                prev_char = if hashes > 0 { '#' } else { '"' };
                continue;
            }
        }

        if ch == '"' {
            i += 1;
            while i < n {
                if chars[i] == '\n' {
                    line += 1;
                } else if chars[i] == '\\' {
                    i += 1;
                    if i < n && chars[i] == '\n' {
                        line += 1;
                    }
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            prev_char = '"';
            continue;
        }

        if ch == '/' && i + 1 < n && chars[i + 1] == '/' {
            let start = i;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            let snippet: String = chars[start..i].iter().collect();
            violations.push((line, format!("Line comment: {}", snippet.trim())));
            prev_char = ' ';
            continue;
        }

        if ch == '/' && i + 1 < n && chars[i + 1] == '*' {
            let start_line = line;
            i += 2;
            let mut depth = 1;
            while i < n && depth > 0 {
                if chars[i] == '\n' {
                    line += 1;
                } else if chars[i..].starts_with(&['/', '*']) {
                    depth += 1;
                    i += 1;
                } else if chars[i..].starts_with(&['*', '/']) {
                    depth -= 1;
                    i += 1;
                }
                i += 1;
            }
            violations.push((start_line, "Block comment: /* ... */".to_string()));
            prev_char = ' ';
            continue;
        }

        if ch == '#' {
            let sample_len = (n - i).min(60);
            let sample: String = chars[i..i + sample_len]
                .iter()
                .filter(|c| !c.is_whitespace())
                .collect();
            if sample.starts_with("#[doc") || sample.starts_with("#![doc") {
                violations.push((line, format!("Doc attribute violation: {}", sample)));
            } else if sample.starts_with("#[rustfmt::skip")
                || sample.starts_with("#![rustfmt::skip")
            {
                violations.push((line, format!("Formatting suppression: {}", sample)));
            }
        }

        if ch == 'i' && !is_ident {
            let sample_len = (n - i).min(20);
            let sample: String = chars[i..i + sample_len].iter().collect();
            if sample.starts_with("include!(")
                || sample.starts_with("include_str!(")
                || sample.starts_with("include_bytes!(")
            {
                violations.push((line, format!("External include macro: {}", sample)));
            }
        }

        prev_char = ch;
        i += 1;
    }

    violations
}

#[test]
fn test_ceiling_500_lines_per_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let targets = collect_rs_files(root);
    assert!(
        targets.len() >= 45,
        "Expected at least 45 Rust source files, found {}",
        targets.len()
    );

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
    let targets = collect_rs_files(root);
    assert!(
        targets.len() >= 45,
        "Expected at least 45 Rust source files, found {}",
        targets.len()
    );

    let mut violations = Vec::new();
    for file in targets {
        let content = fs::read_to_string(&file).unwrap();
        let file_violations = scan_rust_source_violations(&content);
        for (line, desc) in file_violations {
            violations.push(format!("{}:{}: {}", file.display(), line, desc));
        }
    }

    assert!(
        violations.is_empty(),
        "Zero-comment rule violated in the following files:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_stateful_scanner_adversarial_matrix() {
    let pos1 = concat!("let c = '", '"', "'; // hidden comment");
    assert!(!scan_rust_source_violations(pos1).is_empty());

    let pos2 = "let r = r#\" \" \"#; // hidden comment";
    assert!(!scan_rust_source_violations(pos2).is_empty());

    let pos3 = "let s = \"hello\nworld\"; // second line comment";
    assert!(!scan_rust_source_violations(pos3).is_empty());

    let pos4 = "/* multi\nline\nblock\ncomment */";
    assert!(!scan_rust_source_violations(pos4).is_empty());

    let pos5 = "#[ doc = \"doc text\" ]";
    assert!(!scan_rust_source_violations(pos5).is_empty());

    let pos6 = "#![doc = \"inner doc text\"]";
    assert!(!scan_rust_source_violations(pos6).is_empty());

    let pos7 = "#[rustfmt::skip]";
    assert!(!scan_rust_source_violations(pos7).is_empty());

    let pos8 = "include!(\"external.rs\");";
    assert!(!scan_rust_source_violations(pos8).is_empty());

    let neg1 = "let s = \"// not a comment\";";
    assert!(scan_rust_source_violations(neg1).is_empty());

    let neg2 = "let r = r#\" // not a comment \"#;";
    assert!(scan_rust_source_violations(neg2).is_empty());

    let neg3 = "let c = '/';";
    assert!(scan_rust_source_violations(neg3).is_empty());

    let neg4 = "let raw = r#\"{\"event\":\"init\"}\"#;";
    assert!(scan_rust_source_violations(neg4).is_empty());
}

#[test]
fn test_documentation_and_metadata_sync() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let version = env!("CARGO_PKG_VERSION");

    let agents_path = root.join("AGENTS.md");
    let readme_path = root.join("README.md");
    let agents_content = fs::read_to_string(&agents_path).unwrap();
    let readme_content = fs::read_to_string(&readme_path).unwrap();

    assert!(
        agents_content.match_indices(version).count() >= 2,
        "AGENTS.md must contain active package version {} in header and semver section",
        version
    );
    assert!(
        readme_content.contains(version),
        "README.md does not contain active package version {}",
        version
    );

    let patch_series = match version.rfind('.') {
        Some(idx) => format!("{}.x", &version[..idx]),
        None => format!("{}.x", version),
    };
    assert!(
        agents_content.contains(&patch_series),
        "AGENTS.md missing patch series token {}",
        patch_series
    );

    let wiki_dir = root.join(".wiki");
    let arch_wiki_content = if wiki_dir.exists() {
        let arch_wiki = wiki_dir.join("Frontal-Architecture.md");
        if arch_wiki.exists() {
            let content = fs::read_to_string(&arch_wiki).unwrap();
            assert!(
                content.contains(version),
                "Frontal-Architecture.md does not contain active package version {}",
                version
            );
            Some(content)
        } else {
            None
        }
    } else {
        None
    };

    if wiki_dir.exists() {
        let home_wiki = wiki_dir.join("Home.md");
        if home_wiki.exists() {
            let content = fs::read_to_string(&home_wiki).unwrap();
            assert!(
                content.contains(version),
                "Home.md does not contain active package version {}",
                version
            );
        }
        let roadmap_wiki = wiki_dir.join("Board-Roadmap.md");
        if roadmap_wiki.exists() {
            let content = fs::read_to_string(&roadmap_wiki).unwrap();
            assert!(
                content.contains(version),
                "Board-Roadmap.md does not contain active package version {}",
                version
            );
        }
    }

    let extinct_paths = [
        "tests/ui_polish_gate.rs",
        "< 400 linhas",
        "< 400 lines",
        " testes",
        "Fase 1",
        "Fase 2",
        "Fase 3",
        "Fase 4",
        "Fase 5",
        "Fases 1",
        "Phase Dumps",
    ];
    for extinct in extinct_paths {
        assert!(
            !readme_content.contains(extinct),
            "README.md references extinct token: {}",
            extinct
        );
        assert!(
            !agents_content.contains(extinct),
            "AGENTS.md references extinct token: {}",
            extinct
        );
        if wiki_dir.exists() {
            for entry in fs::read_dir(&wiki_dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    let content = fs::read_to_string(&path).unwrap();
                    assert!(
                        !content.contains(extinct),
                        "{} references extinct token: {}",
                        path.display(),
                        extinct
                    );
                }
            }
        }
    }

    let mut total_tests = 0usize;
    let mut domain_counts = std::collections::HashMap::new();
    let test_domains_dir = root.join("tests").join("domains");
    let rs_files = collect_rs_files(&test_domains_dir);

    for file in rs_files {
        let file_name = file.file_name().unwrap().to_str().unwrap();
        if file_name == "mod.rs" {
            continue;
        }
        let content = fs::read_to_string(&file).unwrap();
        let count = content
            .lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("#[test]") || t.starts_with("#[tokio::test]")
            })
            .count();

        total_tests += count;
        let parent_name = file
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let domain_key = if parent_name == "domains" {
            file_name.to_string()
        } else {
            parent_name.to_string()
        };
        *domain_counts.entry(domain_key).or_insert(0) += count;
    }

    let expected_total_str = format!("{} tests", total_tests);
    assert!(
        agents_content.match_indices(&expected_total_str).count() >= 2,
        "AGENTS.md missing dynamic test count token {} in both header and verification sections",
        expected_total_str
    );
    assert!(
        readme_content.contains(&expected_total_str),
        "README.md missing dynamic test count token: {}",
        expected_total_str
    );

    if wiki_dir.exists() {
        if let Some(arch_content) = arch_wiki_content.as_ref() {
            assert!(
                arch_content.contains(&expected_total_str),
                "Frontal-Architecture.md missing dynamic test count token: {}",
                expected_total_str
            );
        }
        let home_wiki = wiki_dir.join("Home.md");
        if home_wiki.exists() {
            let content = fs::read_to_string(&home_wiki).unwrap();
            assert!(
                content.contains(&expected_total_str),
                "Home.md missing dynamic test count token: {}",
                expected_total_str
            );
        }
        let roadmap_wiki = wiki_dir.join("Board-Roadmap.md");
        if roadmap_wiki.exists() {
            let content = fs::read_to_string(&roadmap_wiki).unwrap();
            assert!(
                content.contains(&expected_total_str),
                "Board-Roadmap.md missing dynamic test count token: {}",
                expected_total_str
            );
        }
    }

    for (domain_key, count) in &domain_counts {
        let expected_domain_str = format!("{} tests", count);
        let path_token = if domain_key.ends_with(".rs") {
            format!("tests/domains/{}", domain_key)
        } else {
            format!("tests/domains/{}/", domain_key)
        };
        assert!(
            agents_content.contains(&path_token),
            "AGENTS.md missing path token: {}",
            path_token
        );
        let domain_line = agents_content
            .lines()
            .find(|l| l.contains(&path_token))
            .unwrap_or("");
        assert!(
            domain_line.contains(&expected_domain_str),
            "AGENTS.md domain {} count mismatch: expected {}",
            path_token,
            expected_domain_str
        );

        if let Some(arch_content) = arch_wiki_content.as_ref() {
            let arch_line = arch_content
                .lines()
                .find(|l| l.contains(&path_token))
                .unwrap_or("");
            assert!(
                arch_line.contains(&expected_domain_str),
                "Frontal-Architecture.md domain {} count mismatch: expected {}",
                path_token,
                expected_domain_str
            );
        }
    }
}
