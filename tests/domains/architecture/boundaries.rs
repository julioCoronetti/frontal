use super::collect_rs_files;
use super::lexer::{
    has_ident, has_path_or_grouped_use, has_super_escape, tokenize_rust_source, CodeToken,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BoundaryRule {
    CoreRusqlite,
    CoreTokioProcess,
    CoreStdProcess,
    CoreTokioFs,
    CoreStdFs,
    CoreLibc,
    CoreInfraRef,
    CoreUiRef,
    InfraUiRef,
    UiRusqlite,
    UiTokioProcess,
    UiStdProcess,
    UiTokioFs,
    UiStdFs,
    UiLibc,
    UiHardcodedDbPath,
    UiHardcodedTranscriptPath,
    UiSesPrefixHeuristic,
    UiStaticMutexOrRwLock,
}

fn has_static_mutex_or_rwlock(tokens: &[CodeToken]) -> bool {
    let mut i = 0;
    while i < tokens.len() {
        let is_non_item = i > 0
            && matches!(
                tokens[i - 1],
                CodeToken::Punct('\'') | CodeToken::Punct('#')
            );
        if !is_non_item && matches!(&tokens[i], CodeToken::Ident(id) if id == "static") {
            let mut j = i + 1;
            let mut brace_depth = 0usize;
            while j < tokens.len() {
                match &tokens[j] {
                    CodeToken::Punct('{') => brace_depth += 1,
                    CodeToken::Punct('}') => brace_depth = brace_depth.saturating_sub(1),
                    CodeToken::Punct(';') if brace_depth == 0 => break,
                    CodeToken::Ident(id) if id == "Mutex" || id == "RwLock" => return true,
                    _ => {}
                }
                j += 1;
            }
        }
        i += 1;
    }
    false
}

pub fn scan_core_violations(content: &str) -> BTreeSet<BoundaryRule> {
    let parsed = tokenize_rust_source(content);
    let mut found = BTreeSet::new();
    if has_ident(&parsed.tokens, "rusqlite") {
        found.insert(BoundaryRule::CoreRusqlite);
    }
    if has_path_or_grouped_use(&parsed.tokens, "tokio", "process") {
        found.insert(BoundaryRule::CoreTokioProcess);
    }
    if has_path_or_grouped_use(&parsed.tokens, "std", "process") {
        found.insert(BoundaryRule::CoreStdProcess);
    }
    if has_path_or_grouped_use(&parsed.tokens, "tokio", "fs") {
        found.insert(BoundaryRule::CoreTokioFs);
    }
    if has_path_or_grouped_use(&parsed.tokens, "std", "fs") {
        found.insert(BoundaryRule::CoreStdFs);
    }
    if has_ident(&parsed.tokens, "libc") {
        found.insert(BoundaryRule::CoreLibc);
    }
    if has_path_or_grouped_use(&parsed.tokens, "crate", "infra")
        || has_super_escape(&parsed.tokens, "infra")
    {
        found.insert(BoundaryRule::CoreInfraRef);
    }
    if has_path_or_grouped_use(&parsed.tokens, "crate", "ui")
        || has_super_escape(&parsed.tokens, "ui")
    {
        found.insert(BoundaryRule::CoreUiRef);
    }
    found
}

pub fn scan_infra_violations(content: &str) -> BTreeSet<BoundaryRule> {
    let parsed = tokenize_rust_source(content);
    let mut found = BTreeSet::new();
    if has_path_or_grouped_use(&parsed.tokens, "crate", "ui")
        || has_super_escape(&parsed.tokens, "ui")
    {
        found.insert(BoundaryRule::InfraUiRef);
    }
    found
}

pub fn scan_ui_violations(content: &str) -> BTreeSet<BoundaryRule> {
    let parsed = tokenize_rust_source(content);
    let mut found = BTreeSet::new();
    if has_ident(&parsed.tokens, "rusqlite") {
        found.insert(BoundaryRule::UiRusqlite);
    }
    if has_path_or_grouped_use(&parsed.tokens, "tokio", "process") {
        found.insert(BoundaryRule::UiTokioProcess);
    }
    if has_path_or_grouped_use(&parsed.tokens, "std", "process") {
        found.insert(BoundaryRule::UiStdProcess);
    }
    if has_path_or_grouped_use(&parsed.tokens, "tokio", "fs") {
        found.insert(BoundaryRule::UiTokioFs);
    }
    if has_path_or_grouped_use(&parsed.tokens, "std", "fs") {
        found.insert(BoundaryRule::UiStdFs);
    }
    if has_ident(&parsed.tokens, "libc") {
        found.insert(BoundaryRule::UiLibc);
    }
    if has_static_mutex_or_rwlock(&parsed.tokens) {
        found.insert(BoundaryRule::UiStaticMutexOrRwLock);
    }
    for lit in &parsed.string_literals {
        if lit.contains("opencode.db") {
            found.insert(BoundaryRule::UiHardcodedDbPath);
        }
        if lit.contains("transcript.jsonl") {
            found.insert(BoundaryRule::UiHardcodedTranscriptPath);
        }
        if lit.starts_with("ses_") {
            found.insert(BoundaryRule::UiSesPrefixHeuristic);
        }
    }
    found
}

const LEGACY_ALLOWLIST: &[(&str, BoundaryRule)] = &[
    ("src/core/config.rs", BoundaryRule::CoreStdFs),
    ("src/core/model_catalog.rs", BoundaryRule::CoreRusqlite),
    ("src/core/protocol/mod.rs", BoundaryRule::CoreStdFs),
    ("src/ui/app/respawn.rs", BoundaryRule::UiSesPrefixHeuristic),
    ("src/ui/autocomplete.rs", BoundaryRule::UiStdFs),
    ("src/ui/history.rs", BoundaryRule::UiStdFs),
    ("src/ui/history.rs", BoundaryRule::UiSesPrefixHeuristic),
    ("src/ui/modal/usage/cli.rs", BoundaryRule::UiStdProcess),
    ("src/ui/modal/usage/cli.rs", BoundaryRule::UiLibc),
    ("src/ui/modal/usage/cli.rs", BoundaryRule::UiStdFs),
    ("src/ui/modal/usage/fetch.rs", BoundaryRule::UiStdFs),
    (
        "src/ui/modal/usage/fetch.rs",
        BoundaryRule::UiStaticMutexOrRwLock,
    ),
    (
        "src/ui/modal/usage/opencode_api.rs",
        BoundaryRule::UiStdProcess,
    ),
    (
        "src/ui/modal/usage/opencode_api.rs",
        BoundaryRule::UiRusqlite,
    ),
    (
        "src/ui/modal/usage/opencode_api.rs",
        BoundaryRule::UiHardcodedDbPath,
    ),
    (
        "src/ui/modal/usage/opencode_api.rs",
        BoundaryRule::UiStaticMutexOrRwLock,
    ),
    (
        "src/ui/modal/usage/stats.rs",
        BoundaryRule::UiStaticMutexOrRwLock,
    ),
    (
        "src/ui/modal/usage/types.rs",
        BoundaryRule::UiStaticMutexOrRwLock,
    ),
];

fn scan_layer_dir(
    root: &Path,
    rel_dir: &str,
    scanner: fn(&str) -> BTreeSet<BoundaryRule>,
    observed: &mut BTreeSet<(String, BoundaryRule)>,
) {
    for file in collect_rs_files(&root.join(rel_dir)) {
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let content = fs::read_to_string(&file).unwrap();
        for rule in scanner(&content) {
            observed.insert((rel.clone(), rule));
        }
    }
}

#[test]
fn test_layer_boundaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let allowlist: BTreeSet<(&str, BoundaryRule)> = LEGACY_ALLOWLIST.iter().copied().collect();
    let mut observed: BTreeSet<(String, BoundaryRule)> = BTreeSet::new();

    scan_layer_dir(root, "src/core", scan_core_violations, &mut observed);
    scan_layer_dir(root, "src/infra", scan_infra_violations, &mut observed);
    scan_layer_dir(root, "src/ui", scan_ui_violations, &mut observed);

    let unexpected: Vec<String> = observed
        .iter()
        .filter(|(p, r)| !allowlist.contains(&(p.as_str(), *r)))
        .map(|(p, r)| format!("{}: {:?}", p, r))
        .collect();

    let stale: Vec<String> = allowlist
        .iter()
        .filter(|(p, r)| !observed.contains(&((*p).to_string(), *r)))
        .map(|(p, r)| format!("{}: {:?}", p, r))
        .collect();

    assert!(
        unexpected.is_empty() && stale.is_empty(),
        "Layer boundary ratchet failed.\nUnexpected violations:\n{}\nStale allowlist entries (remove them):\n{}",
        unexpected.join("\n"),
        stale.join("\n")
    );
}

#[test]
fn test_boundary_scanner_adversarial_matrix() {
    let grouped = "use std::{env, fs};\nuse tokio::{fs as tfs, process::Command};";
    let core_grouped = scan_core_violations(grouped);
    assert!(core_grouped.contains(&BoundaryRule::CoreStdFs));
    assert!(core_grouped.contains(&BoundaryRule::CoreTokioFs));
    assert!(core_grouped.contains(&BoundaryRule::CoreTokioProcess));

    let precision_neg =
        "use crate::{core::{ui}};\nuse crate::{core::ui};\nuse std::{env as fs};\nuse super::{submod::{ui}};\nuse super::{foo as ui};";
    assert!(scan_core_violations(precision_neg).is_empty());

    let precision_pos = "use crate::{core::{a, b}, ui};\nuse std::{io::Read, fs::File};";
    let core_pos = scan_core_violations(precision_pos);
    assert!(core_pos.contains(&BoundaryRule::CoreUiRef));
    assert!(core_pos.contains(&BoundaryRule::CoreStdFs));

    let super_escape = "use super::super::infra::db;\nuse super::ui::theme;";
    let core_super = scan_core_violations(super_escape);
    assert!(core_super.contains(&BoundaryRule::CoreInfraRef));
    assert!(core_super.contains(&BoundaryRule::CoreUiRef));

    assert!(scan_infra_violations("use crate::{core, ui};").contains(&BoundaryRule::InfraUiRef));
    assert!(scan_infra_violations("use super::ui::modal;").contains(&BoundaryRule::InfraUiRef));
    assert!(scan_infra_violations("let s = \"crate::ui\";").is_empty());

    let ignored_comments_and_raw = concat!(
        "// crate::ui\n",
        "/* /* std::fs crate::infra */ */\n",
        "let raw = r#\"crate::ui std::fs\"#;"
    );
    assert!(scan_core_violations(ignored_comments_and_raw).is_empty());
    assert!(scan_infra_violations(ignored_comments_and_raw).is_empty());

    let literal_only = "let s = \"std::fs rusqlite crate::infra libc\";";
    assert!(scan_core_violations(literal_only).is_empty());

    let ui_sample = concat!(
        "static CACHE: std::sync::LazyLock<\n",
        "    std::sync::Mutex<Vec<u8>>\n",
        "> = std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));\n",
        "let db = \"opencode.db\";\n",
        "let tr = \"transcript.jsonl\";\n",
        "let p = \"ses_\";"
    );
    let ui_hits = scan_ui_violations(ui_sample);
    assert!(ui_hits.contains(&BoundaryRule::UiStaticMutexOrRwLock));
    assert!(ui_hits.contains(&BoundaryRule::UiHardcodedDbPath));
    assert!(ui_hits.contains(&BoundaryRule::UiHardcodedTranscriptPath));
    assert!(ui_hits.contains(&BoundaryRule::UiSesPrefixHeuristic));

    let static_lifetime =
        "fn spawn<F: Send + 'static>(f: F) { let _m = std::sync::Mutex::new(0); }";
    assert!(!scan_ui_violations(static_lifetime).contains(&BoundaryRule::UiStaticMutexOrRwLock));

    assert!(scan_ui_violations("// opencode.db\n/* ses_ */").is_empty());
    assert!(scan_ui_violations("let s = r#\"opencode.db\"#;")
        .contains(&BoundaryRule::UiHardcodedDbPath));
    assert!(scan_core_violations("use super::").is_empty());
    assert!(!scan_ui_violations("let r#static = Mutex::new(0);")
        .contains(&BoundaryRule::UiStaticMutexOrRwLock));
    assert!(scan_ui_violations("let s = \"ses_{}\";").contains(&BoundaryRule::UiSesPrefixHeuristic));
}
