use super::sql::{check_sql_statement, is_likely_sql};
use super::{danger_assessment, DangerAssessment, RiskCategory};

pub fn evaluate_command(raw: &str) -> Option<DangerAssessment> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_likely_sql(trimmed) {
        if let danger @ Some(_) = check_sql_statement(trimmed) {
            return danger;
        }
    }
    split_shell_segments(trimmed)
        .into_iter()
        .find_map(|seg| evaluate_shell_segment(seg, trimmed))
}

pub fn split_shell_segments(cmd: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let (mut in_single, mut in_double, mut escaped) = (false, false, false);
    let mut start = 0;
    let chars: Vec<(usize, char)> = cmd.char_indices().collect();
    let n = chars.len();
    let mut idx = 0;

    while idx < n {
        let (byte_pos, ch) = chars[idx];
        if std::mem::take(&mut escaped) {
            idx += 1;
            continue;
        }
        if ch == '\\' && !in_single {
            escaped = true;
            idx += 1;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '"' && !in_single {
            in_double = !in_double;
        } else if !in_single && !in_double {
            let is_and_or = (ch == '&' || ch == '|') && idx + 1 < n && chars[idx + 1].1 == ch;
            if is_and_or || ch == ';' || ch == '\n' || ch == '|' {
                let seg = cmd[start..byte_pos].trim();
                if !seg.is_empty() {
                    segments.push(seg);
                }
                if is_and_or {
                    start = chars[idx + 1].0 + chars[idx + 1].1.len_utf8();
                    idx += 2;
                    continue;
                }
                start = byte_pos + ch.len_utf8();
            }
        }
        idx += 1;
    }
    let tail = cmd[start..].trim();
    if !tail.is_empty() {
        segments.push(tail);
    }
    segments
}

pub fn tokenize_words(segment: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let (mut in_single, mut in_double, mut escaped) = (false, false, false);

    for ch in segment.chars() {
        if std::mem::take(&mut escaped) {
            current.push(ch);
        } else if ch == '\\' && !in_single {
            escaped = true;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '"' && !in_single {
            in_double = !in_double;
        } else if ch.is_whitespace() && !in_single && !in_double {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn evaluate_shell_segment(seg: &str, full_cmd: &str) -> Option<DangerAssessment> {
    let raw_words = tokenize_words(seg);
    if raw_words.is_empty() {
        return None;
    }

    let mut idx = 0;
    while idx < raw_words.len() && raw_words[idx].contains('=') && !raw_words[idx].starts_with('-')
    {
        idx += 1;
    }
    while idx < raw_words.len() {
        let base = extract_basename(&raw_words[idx]);
        if !matches!(
            base.as_str(),
            "sudo" | "env" | "nohup" | "time" | "command" | "exec"
        ) {
            break;
        }
        idx += 1;
        while idx < raw_words.len() && raw_words[idx].starts_with('-') {
            idx += 1;
        }
    }
    if idx >= raw_words.len() {
        return None;
    }

    let exe = extract_basename(&raw_words[idx]);
    let args = &raw_words[idx + 1..];

    if is_safe_read_command(&exe) {
        return None;
    }
    if matches!(exe.as_str(), "sh" | "bash" | "zsh" | "dash") && args.len() >= 2 && args[0] == "-c"
    {
        return evaluate_command(&args[1]);
    }
    if exe == "rm"
        && args.iter().any(|a| {
            a == "--recursive"
                || (a.starts_with('-')
                    && !a.starts_with("--")
                    && (a.contains('r') || a.contains('R')))
        })
    {
        return Some(danger_assessment(
            RiskCategory::RecursiveDeletion,
            full_cmd,
            first_positional_arg(args, "filesystem paths"),
            "Recursive file or directory deletion",
        ));
    }
    if exe == "rmdir" {
        return Some(danger_assessment(
            RiskCategory::RecursiveDeletion,
            full_cmd,
            first_positional_arg(args, "directory"),
            "Directory removal",
        ));
    }
    if exe == "git" && !args.is_empty() {
        return evaluate_git_command(args, full_cmd);
    }
    if matches!(
        exe.as_str(),
        "psql" | "sqlite3" | "sqlite" | "mysql" | "duckdb" | "mariadb"
    ) {
        for arg in args {
            if let Some(danger) = check_sql_statement(arg) {
                return Some(danger_assessment(
                    danger.category,
                    full_cmd,
                    danger.target,
                    "Destructive SQL executed via database CLI",
                ));
            }
        }
    }
    if matches!(exe.as_str(), "dd" | "shred" | "wipefs" | "fdisk" | "parted") {
        let target = args
            .iter()
            .find(|a| a.starts_with("of="))
            .map(|a| a.trim_start_matches("of=").to_string())
            .unwrap_or_else(|| first_positional_arg(args, "storage device"));
        return Some(danger_assessment(
            RiskCategory::DiskManipulation,
            full_cmd,
            target,
            "Direct disk or block device manipulation",
        ));
    }
    if exe.starts_with("mkfs") {
        return Some(danger_assessment(
            RiskCategory::DiskManipulation,
            full_cmd,
            first_positional_arg(args, "target partition"),
            "Filesystem formatting will erase partition data",
        ));
    }
    None
}

fn evaluate_git_command(args: &[String], full_cmd: &str) -> Option<DangerAssessment> {
    let mut i = 0;
    while i < args.len() && args[i].starts_with('-') {
        i += if matches!(
            args[i].as_str(),
            "-C" | "-c" | "--work-tree" | "--git-dir" | "--namespace"
        ) {
            2
        } else {
            1
        };
    }
    if i >= args.len() {
        return None;
    }
    let (sub, sub_args) = (&args[i], &args[i + 1..]);
    if sub == "reset" && sub_args.iter().any(|a| a == "--hard") {
        return Some(danger_assessment(
            RiskCategory::DestructiveVcs,
            full_cmd,
            first_positional_arg(sub_args, "HEAD"),
            "Hard reset will discard all uncommitted changes",
        ));
    }
    if sub == "clean"
        && sub_args
            .iter()
            .any(|a| a == "--force" || (a.starts_with('-') && a.contains('f')))
    {
        return Some(danger_assessment(
            RiskCategory::DestructiveVcs,
            full_cmd,
            String::from("untracked files and directories"),
            "Permanent deletion of untracked workspace files",
        ));
    }
    if sub == "push"
        && sub_args
            .iter()
            .any(|a| (a == "--force" || a == "-f") && a != "--force-with-lease")
    {
        return Some(danger_assessment(
            RiskCategory::DestructiveVcs,
            full_cmd,
            first_positional_arg(sub_args, "remote repository"),
            "Forced push will overwrite remote commit history",
        ));
    }
    if sub == "branch" && sub_args.iter().any(|a| a == "-D") {
        return Some(danger_assessment(
            RiskCategory::DestructiveVcs,
            full_cmd,
            first_positional_arg(sub_args, "branch"),
            "Forced branch deletion",
        ));
    }
    None
}

fn first_positional_arg(args: &[String], default: &'static str) -> String {
    args.iter()
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn extract_basename(path_str: &str) -> String {
    let trimmed = path_str.trim_matches('\'').trim_matches('"');
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

fn is_safe_read_command(exe: &str) -> bool {
    matches!(
        exe,
        "cat"
            | "ls"
            | "grep"
            | "rg"
            | "echo"
            | "printf"
            | "head"
            | "tail"
            | "less"
            | "more"
            | "wc"
            | "stat"
            | "find"
    )
}
