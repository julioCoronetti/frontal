use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::EngineConfig;

pub fn resolve_opencode_binary() -> PathBuf {
    if let Ok(path_str) = env::var("FRONTAL_OPENCODE_BIN") {
        let candidate = PathBuf::from(path_str);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("opencode")
}

pub fn opencode_variant(effort: &str) -> Option<&'static str> {
    match effort.trim().to_lowercase().as_str() {
        "low" => Some("low"),
        "medium" => Some("high"),
        "high" | "xhigh" | "max" => Some("max"),
        _ => None,
    }
}

pub fn model_with_variant(model: &str, effort: Option<&str>) -> String {
    if model.contains('#') {
        return model.to_string();
    }
    match effort.and_then(opencode_variant) {
        Some(variant) => format!("{}#{}", model, variant),
        None => model.to_string(),
    }
}

pub fn build_opencode_args(
    config: &EngineConfig,
    session_id: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--thinking".to_string(),
        "--auto".to_string(),
    ];
    if let Some(model) = config.model.as_deref().filter(|m| !m.is_empty()) {
        args.push("--model".to_string());
        args.push(model_with_variant(model, config.effort.as_deref()));
    }
    if let Some(sid) = session_id.filter(|s| !s.is_empty()) {
        args.push("--session".to_string());
        args.push(sid.to_string());
    }
    args.push("--".to_string());
    args.push(prompt.to_string());
    args
}

pub fn parse_opencode_models_output(raw: &str) -> Vec<(String, String)> {
    let mut models = Vec::new();
    for line in raw.lines() {
        let cleaned = strip_ansi(line);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.split('/').count() != 2 {
            continue;
        }
        let Some((provider, model)) = trimmed.split_once('/') else {
            continue;
        };
        if provider.is_empty() || model.is_empty() || model.contains(char::is_whitespace) {
            continue;
        }
        models.push((trimmed.to_string(), model.to_string()));
    }
    models
}

pub async fn fetch_opencode_models(bin: &Path) -> Vec<(String, String)> {
    let output = match tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new(bin).arg("models").output(),
    )
    .await
    {
        Ok(Ok(out)) if out.status.success() => out.stdout,
        _ => return Vec::new(),
    };
    parse_opencode_models_output(&String::from_utf8_lossy(&output))
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}
