use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskCategory {
    RecursiveDeletion,
    DestructiveVcs,
    DiskManipulation,
    DestructiveSql,
}

impl RiskCategory {
    pub fn title(&self) -> &'static str {
        match self {
            Self::RecursiveDeletion => "Recursive Deletion",
            Self::DestructiveVcs => "Destructive VCS Action",
            Self::DiskManipulation => "Disk Manipulation",
            Self::DestructiveSql => "Destructive SQL Operation",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DangerAssessment {
    pub category: RiskCategory,
    pub command: String,
    pub target: String,
    pub warning: String,
    pub tool_name: String,
    pub step_index: Option<u64>,
}

pub fn evaluate_command(raw: &str) -> Option<DangerAssessment> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(danger) = check_sql_statement(trimmed) {
        return Some(danger);
    }
    for seg in split_shell_segments(trimmed) {
        if let Some(danger) = evaluate_shell_segment(seg, trimmed) {
            return Some(danger);
        }
    }
    None
}

pub fn evaluate_tool_call(
    tool_name: &str,
    step_index: Option<u64>,
    params: Option<&Value>,
    fallback_args: Option<&str>,
) -> Option<DangerAssessment> {
    for cmd in extract_candidate_commands(params, fallback_args) {
        if let Some(mut assessment) = evaluate_command(&cmd) {
            assessment.tool_name = tool_name.to_string();
            assessment.step_index = step_index;
            return Some(assessment);
        }
    }
    None
}

fn split_shell_segments(cmd: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let (mut in_single, mut in_double, mut escaped) = (false, false, false);
    let mut start = 0;
    let chars: Vec<(usize, char)> = cmd.char_indices().collect();
    let n = chars.len();
    let mut idx = 0;

    while idx < n {
        let (byte_pos, ch) = chars[idx];
        if escaped {
            escaped = false;
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

fn tokenize_words(segment: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let (mut in_single, mut in_double, mut escaped) = (false, false, false);

    for ch in segment.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' && !in_single {
            escaped = true;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '"' && !in_single {
            in_double = !in_double;
        } else if ch.is_whitespace() && !in_single && !in_double {
            if !current.is_empty() {
                words.push(current);
                current = String::new();
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

fn evaluate_shell_segment(seg: &str, full_cmd: &str) -> Option<DangerAssessment> {
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
        if matches!(
            base.as_str(),
            "sudo" | "env" | "nohup" | "time" | "command" | "exec"
        ) {
            idx += 1;
            while idx < raw_words.len() && raw_words[idx].starts_with('-') {
                idx += 1;
            }
        } else {
            break;
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
    if exe == "rm" {
        let is_rec = args.iter().any(|a| {
            a == "--recursive"
                || (a.starts_with('-')
                    && !a.starts_with("--")
                    && (a.contains('r') || a.contains('R')))
        });
        if is_rec {
            let target = args
                .iter()
                .find(|a| !a.starts_with('-'))
                .cloned()
                .unwrap_or_else(|| String::from("filesystem paths"));
            return Some(danger_assessment(
                RiskCategory::RecursiveDeletion,
                full_cmd,
                target,
                "Recursive file or directory deletion",
            ));
        }
    }
    if exe == "rmdir" {
        let target = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_else(|| String::from("directory"));
        return Some(danger_assessment(
            RiskCategory::RecursiveDeletion,
            full_cmd,
            target,
            "Directory removal",
        ));
    }
    if exe == "git" && !args.is_empty() {
        return evaluate_git_command(args, full_cmd);
    }
    if matches!(exe.as_str(), "dd" | "shred" | "wipefs" | "fdisk" | "parted") {
        let target = args
            .iter()
            .find(|a| a.starts_with("of="))
            .map(|a| a.trim_start_matches("of=").to_string())
            .or_else(|| args.iter().find(|a| !a.starts_with('-')).cloned())
            .unwrap_or_else(|| String::from("storage device"));
        return Some(danger_assessment(
            RiskCategory::DiskManipulation,
            full_cmd,
            target,
            "Direct disk or block device manipulation",
        ));
    }
    if exe.starts_with("mkfs") {
        let target = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_else(|| String::from("target partition"));
        return Some(danger_assessment(
            RiskCategory::DiskManipulation,
            full_cmd,
            target,
            "Filesystem formatting will erase partition data",
        ));
    }
    None
}

fn evaluate_git_command(args: &[String], full_cmd: &str) -> Option<DangerAssessment> {
    let (sub, sub_args) = (&args[0], &args[1..]);
    if sub == "reset" && sub_args.iter().any(|a| a == "--hard") {
        let target = sub_args
            .iter()
            .find(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_else(|| String::from("HEAD"));
        return Some(danger_assessment(
            RiskCategory::DestructiveVcs,
            full_cmd,
            target,
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
        let target = sub_args
            .iter()
            .find(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_else(|| String::from("remote repository"));
        return Some(danger_assessment(
            RiskCategory::DestructiveVcs,
            full_cmd,
            target,
            "Forced push will overwrite remote commit history",
        ));
    }
    if sub == "branch" && sub_args.iter().any(|a| a == "-D") {
        let target = sub_args
            .iter()
            .find(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_else(|| String::from("branch"));
        return Some(danger_assessment(
            RiskCategory::DestructiveVcs,
            full_cmd,
            target,
            "Forced branch deletion",
        ));
    }
    None
}

fn check_sql_statement(raw: &str) -> Option<DangerAssessment> {
    let tokens = tokenize_sql(raw);
    let n = tokens.len();
    if n < 2 {
        return None;
    }
    for i in 0..n {
        let t1 = tokens[i].to_ascii_uppercase();
        if t1 == "DROP" && i + 1 < n {
            let t2 = tokens[i + 1].to_ascii_uppercase();
            if matches!(t2.as_str(), "DATABASE" | "SCHEMA" | "TABLE" | "VIEW") {
                let target = tokens
                    .get(i + 2)
                    .cloned()
                    .unwrap_or_else(|| String::from("database entity"));
                return Some(danger_assessment(
                    RiskCategory::DestructiveSql,
                    raw,
                    target,
                    "Permanent drop of database schema or table",
                ));
            }
        }
        if t1 == "TRUNCATE" && i + 1 < n {
            let next = tokens[i + 1].to_ascii_uppercase();
            let target = if next == "TABLE" && i + 2 < n {
                tokens[i + 2].clone()
            } else {
                tokens[i + 1].clone()
            };
            return Some(danger_assessment(
                RiskCategory::DestructiveSql,
                raw,
                target,
                "Truncate operation erases all rows irreversibly",
            ));
        }
    }
    None
}

fn tokenize_sql(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let (mut in_single, mut in_double) = (false, false);

    for ch in raw.chars() {
        if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '"' && !in_single {
            in_double = !in_double;
        } else if !in_single && !in_double {
            if ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == '-' {
                current.push(ch);
            } else if !current.is_empty() {
                tokens.push(current);
                current = String::new();
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
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

fn danger_assessment(
    category: RiskCategory,
    command: &str,
    target: String,
    warning: &'static str,
) -> DangerAssessment {
    DangerAssessment {
        category,
        command: command.to_string(),
        target,
        warning: warning.to_string(),
        tool_name: String::new(),
        step_index: None,
    }
}

fn extract_candidate_commands(params: Option<&Value>, fallback: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(val) = params {
        match val {
            Value::String(s) => out.push(s.clone()),
            Value::Object(map) => {
                for key in ["CommandLine", "command", "cmd", "script", "query", "sql"] {
                    if let Some(s) = map.get(key).and_then(|v| v.as_str()) {
                        out.push(s.to_string());
                    }
                }
                for (_, v) in map {
                    if let Some(s) = v.as_str() {
                        if !out.contains(&s.to_string()) {
                            out.push(s.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(fb) = fallback {
        let s = fb.to_string();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}
