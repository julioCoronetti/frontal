pub mod shell;
pub mod sql;

use serde_json::Value;

pub use shell::{evaluate_command, evaluate_shell_segment, split_shell_segments, tokenize_words};
pub use sql::{check_sql_statement, is_likely_sql};

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

pub fn evaluate_tool_call(
    tool_name: &str,
    step_index: Option<u64>,
    params: Option<&Value>,
    fallback_args: Option<&str>,
) -> Option<DangerAssessment> {
    if is_non_execution_tool(tool_name) {
        return None;
    }
    extract_candidate_commands(params, fallback_args)
        .into_iter()
        .find_map(|cmd| {
            evaluate_command(&cmd).map(|mut d| {
                d.tool_name = tool_name.to_string();
                d.step_index = step_index;
                d
            })
        })
}

fn is_non_execution_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file"
            | "write_file"
            | "edit_file"
            | "view_file"
            | "list_dir"
            | "grep_search"
            | "find_by_name"
            | "search_web"
            | "ask_question"
    )
}

pub(crate) fn danger_assessment(
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
            Value::String(s) => {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
            Value::Object(map) => {
                for key in ["CommandLine", "command", "cmd", "script", "query", "sql"] {
                    if let Some(s) = map.get(key).and_then(|v| v.as_str()) {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            out.push(trimmed.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        if let Some(fb) = fallback.filter(|s| !s.trim().is_empty()) {
            out.push(fb.trim().to_string());
        }
    }
    out
}
