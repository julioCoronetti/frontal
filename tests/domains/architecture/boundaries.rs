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
    CoreHardcodedStoragePath,
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
    UiWildcardInfraReexport,
}

fn scan_stmt_end(tokens: &[CodeToken], start: usize) -> Option<usize> {
    let mut j = start;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    while j < tokens.len() {
        match &tokens[j] {
            CodeToken::Punct('{') => brace_depth += 1,
            CodeToken::Punct('}') => brace_depth = brace_depth.saturating_sub(1),
            CodeToken::Punct('[') => bracket_depth += 1,
            CodeToken::Punct(']') => bracket_depth = bracket_depth.saturating_sub(1),
            CodeToken::Punct('(') => paren_depth += 1,
            CodeToken::Punct(')') => paren_depth = paren_depth.saturating_sub(1),
            CodeToken::Punct(';') if brace_depth == 0 && bracket_depth == 0 && paren_depth == 0 => {
                return Some(j);
            }
            _ => {}
        }
        j += 1;
    }
    None
}

fn collect_lock_type_aliases(tokens: &[CodeToken]) -> BTreeSet<String> {
    let mut alias_defs: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let is_attr =
            i > 0 && matches!(tokens[i - 1], CodeToken::Punct('#') | CodeToken::Punct('.'));
        if !is_attr
            && i + 3 < tokens.len()
            && matches!(&tokens[i], CodeToken::Ident(id) if id == "type")
        {
            if let CodeToken::Ident(alias_name) = &tokens[i + 1] {
                if let Some(semi) = scan_stmt_end(tokens, i + 2) {
                    let (mut d, mut eq_idx) = ([0usize; 4], None);
                    for k in i + 2..semi {
                        let top = d[0] == 0 && d[2] == 0 && d[3] == 0;
                        match &tokens[k] {
                            CodeToken::Punct('{') => d[0] += 1,
                            CodeToken::Punct('}') => d[0] = d[0].saturating_sub(1),
                            CodeToken::Punct('<') if top => d[1] += 1,
                            CodeToken::Punct('>')
                                if top
                                    && !matches!(
                                        tokens.get(k - 1),
                                        Some(CodeToken::Punct('-'))
                                    ) =>
                            {
                                d[1] = d[1].saturating_sub(1);
                            }
                            CodeToken::Punct('(') => d[2] += 1,
                            CodeToken::Punct(')') => d[2] = d[2].saturating_sub(1),
                            CodeToken::Punct('[') => d[3] += 1,
                            CodeToken::Punct(']') => d[3] = d[3].saturating_sub(1),
                            CodeToken::Punct('=') if top && d[1] == 0 => {
                                eq_idx = Some(k);
                                break;
                            }
                            _ => {}
                        }
                    }
                    if let Some(eq) = eq_idx {
                        let rhs_idents = tokens[eq + 1..semi]
                            .iter()
                            .filter_map(|t| match t {
                                CodeToken::Ident(id) => Some(id.clone()),
                                _ => None,
                            })
                            .collect();
                        alias_defs.push((alias_name.clone(), rhs_idents));
                    }
                }
            }
        } else if !is_attr && matches!(&tokens[i], CodeToken::Ident(id) if id == "use") {
            if let Some(semi) = scan_stmt_end(tokens, i + 1) {
                for k in i + 1..semi.saturating_sub(2) {
                    if let (
                        CodeToken::Ident(src),
                        CodeToken::Ident(as_kw),
                        CodeToken::Ident(alias),
                    ) = (&tokens[k], &tokens[k + 1], &tokens[k + 2])
                    {
                        if as_kw == "as" && alias != "_" {
                            alias_defs.push((alias.clone(), vec![src.clone()]));
                        }
                    }
                }
                i = semi;
            }
        }
        i += 1;
    }

    let mut lock_aliases = BTreeSet::new();
    loop {
        let mut added = false;
        for (name, rhs) in &alias_defs {
            if !lock_aliases.contains(name)
                && rhs
                    .iter()
                    .any(|id| id == "Mutex" || id == "RwLock" || lock_aliases.contains(id))
            {
                lock_aliases.insert(name.clone());
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    lock_aliases
}

fn has_static_mutex_or_rwlock(tokens: &[CodeToken]) -> bool {
    let lock_aliases = collect_lock_type_aliases(tokens);
    let mut i = 0;
    while i < tokens.len() {
        let is_non_item = i > 0
            && matches!(
                tokens[i - 1],
                CodeToken::Punct('\'') | CodeToken::Punct('#')
            );
        if !is_non_item && matches!(&tokens[i], CodeToken::Ident(id) if id == "static") {
            let end = scan_stmt_end(tokens, i + 1).unwrap_or(tokens.len());
            if tokens[i + 1..end].iter().any(|t| {
                matches!(
                    t,
                    CodeToken::Ident(id)
                        if id == "Mutex" || id == "RwLock" || lock_aliases.contains(id)
                )
            }) {
                return true;
            }
            i = end;
        }
        i += 1;
    }
    false
}

fn stmt_has_wildcard_infra_reexport(stmt: &[CodeToken]) -> bool {
    let mut stack: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for tok in stmt {
        match tok {
            CodeToken::Ident(id) => cur.push(id.clone()),
            CodeToken::DoubleColon => {}
            CodeToken::Punct('{') => {
                stack.push(std::mem::take(&mut cur));
            }
            CodeToken::Punct(',') => cur.clear(),
            CodeToken::Punct('}') => {
                cur = stack.pop().unwrap_or_default();
            }
            CodeToken::Punct('*') => {
                let mut full: Vec<&str> = stack
                    .iter()
                    .flatten()
                    .chain(&cur)
                    .map(String::as_str)
                    .collect();
                full.retain(|s| !s.is_empty());
                let is_crate_infra = full.starts_with(&["crate", "infra"]);
                let is_super_infra = full
                    .iter()
                    .position(|&s| s != "super")
                    .is_some_and(|idx| idx > 0 && full[idx] == "infra");
                if is_crate_infra || is_super_infra {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn has_wildcard_infra_pub_use(tokens: &[CodeToken]) -> bool {
    let mut i = 0;
    while i + 1 < tokens.len() {
        let is_attr =
            i > 0 && matches!(tokens[i - 1], CodeToken::Punct('#') | CodeToken::Punct('.'));
        if !is_attr && matches!(&tokens[i], CodeToken::Ident(p) if p == "pub") {
            let mut u_idx = i + 1;
            if matches!(tokens.get(u_idx), Some(CodeToken::Punct('('))) {
                let mut paren_depth = 0usize;
                while u_idx < tokens.len() {
                    match tokens[u_idx] {
                        CodeToken::Punct('(') => paren_depth += 1,
                        CodeToken::Punct(')') => {
                            paren_depth = paren_depth.saturating_sub(1);
                            u_idx += 1;
                            if paren_depth == 0 {
                                break;
                            }
                            continue;
                        }
                        _ => {}
                    }
                    u_idx += 1;
                }
            }
            if matches!(tokens.get(u_idx), Some(CodeToken::Ident(u)) if u == "use") {
                let start = u_idx + 1;
                let end = scan_stmt_end(tokens, start).unwrap_or(tokens.len());
                if stmt_has_wildcard_infra_reexport(&tokens[start..end]) {
                    return true;
                }
                i = end;
            }
        }
        i += 1;
    }
    false
}

fn contains_storage_marker(lit: &str) -> bool {
    lit.contains("opencode.db")
        || lit.contains("history.db")
        || lit.contains("conversation_summaries.db")
        || lit.contains("antigravity-oauth-token")
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
    for lit in &parsed.string_literals {
        if contains_storage_marker(lit) || lit.contains("transcript.jsonl") {
            found.insert(BoundaryRule::CoreHardcodedStoragePath);
        }
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
    if has_wildcard_infra_pub_use(&parsed.tokens) {
        found.insert(BoundaryRule::UiWildcardInfraReexport);
    }
    for lit in &parsed.string_literals {
        if contains_storage_marker(lit) {
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

const LEGACY_ALLOWLIST: &[(&str, BoundaryRule)] =
    &[("src/ui/app/respawn.rs", BoundaryRule::UiSesPrefixHeuristic)];

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

    let ui_sample = "static CACHE: std::sync::LazyLock<std::sync::Mutex<Vec<u8>>> = std::sync::LazyLock::new(Default::default);\nlet (db, tr, p) = (\"opencode.db\", \"transcript.jsonl\", \"ses_\");";
    let ui_hits = scan_ui_violations(ui_sample);
    assert!(ui_hits.contains(&BoundaryRule::UiStaticMutexOrRwLock));
    assert!(ui_hits.contains(&BoundaryRule::UiHardcodedDbPath));
    assert!(ui_hits.contains(&BoundaryRule::UiHardcodedTranscriptPath));
    assert!(ui_hits.contains(&BoundaryRule::UiSesPrefixHeuristic));

    for ok_static in [
        "fn spawn<F: Send + 'static>(f: F) { let _m = std::sync::Mutex::new(0); }",
        "let r#static = Mutex::new(0);",
        "fn f(x: usize) { let _m = x as Mutex; }",
    ] {
        assert!(!scan_ui_violations(ok_static).contains(&BoundaryRule::UiStaticMutexOrRwLock));
    }
    assert!(scan_ui_violations("// opencode.db\n/* ses_ */").is_empty());
    assert!(scan_ui_violations("let s = r#\"opencode.db\"#;")
        .contains(&BoundaryRule::UiHardcodedDbPath));
    assert!(scan_core_violations("use super::").is_empty());
    assert!(scan_ui_violations("let s = \"ses_{}\";").contains(&BoundaryRule::UiSesPrefixHeuristic));
    assert!(scan_ui_violations("pub use crate::{infra::A, core::*};").is_empty());
    assert!(scan_ui_violations("pub use crate::infra::history::PromptHistory;").is_empty());

    for snippet in [
        "type InnerLock = std::sync::Mutex<Vec<u8>>;\ntype OuterLock = std::sync::Arc<InnerLock>;\nstatic ALIASED: std::sync::LazyLock<OuterLock> = std::sync::LazyLock::new(Default::default);",
        "use std::sync::RwLock as SyncRwLock;\nstatic RW: SyncRwLock<u8> = SyncRwLock::new(0);",
        "type A = ([u8; 1], std::sync::Mutex<u8>);\nstatic X: A = todo!();",
        "static X: ([u8; 1], std::sync::Mutex<u8>) = todo!();",
        "use std::sync::{Mutex as M};\nstatic X: M<u8> = M::new(0);",
        "static X: () = { let _ = 1; let _m: std::sync::Mutex<u8>; };",
        "type T<A: Fn() -> u8, B: Tr<X = u8>> = std::sync::Mutex<(A, B)>;\nstatic X: T<u8, u8> = todo!();",
        "type B<const N: usize = { (1 << 2) >> 1 }> = std::sync::Mutex<[u8; N]>;\nstatic X: B = todo!();",
        "type W<A> where [(); { let k = match 0 { _ => 1 }; k }]: Send = std::sync::Mutex<A>;\nstatic X: W<u8> = todo!();",
    ] {
        assert!(scan_ui_violations(snippet).contains(&BoundaryRule::UiStaticMutexOrRwLock));
    }

    for snippet in [
        "pub(crate) use crate::infra::*;",
        "pub use super::infra::*;",
        "pub(in crate::ui) use crate::infra::*;",
        "pub use crate::{core::A, infra::*};",
        "pub use crate::infra::history::*;",
    ] {
        assert!(scan_ui_violations(snippet).contains(&BoundaryRule::UiWildcardInfraReexport));
    }

    let core_storage =
        "let p = \"opencode.db\"; let t = \"antigravity-oauth-token\"; let l = \"transcript.jsonl\";";
    assert!(scan_core_violations(core_storage).contains(&BoundaryRule::CoreHardcodedStoragePath));

    for marker in [
        "history.db",
        "conversation_summaries.db",
        "antigravity-oauth-token",
    ] {
        assert!(scan_ui_violations(&format!("let s = \"{}\";", marker))
            .contains(&BoundaryRule::UiHardcodedDbPath));
    }
}
