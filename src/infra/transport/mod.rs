use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod opencode;
mod replay;

pub use opencode::{
    build_opencode_args, build_opencode_payload_args, delete_opencode_session,
    fetch_opencode_models, is_valid_opencode_session_id, model_with_variant, opencode_variant,
    parse_opencode_models_output, resolve_opencode_binary, run_opencode_command,
    run_opencode_command_with_bin, strip_ansi,
};
pub use replay::{FixtureReplayEngine, ReplayMode};

use crate::config::BackendKind;
use crate::protocol::ProtocolError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    Io(String),
    ProcessNotFound(String),
    ProcessNotRunning,
    Protocol(String),
    Timeout,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "IO error: {}", err),
            Self::ProcessNotFound(bin) => write!(f, "Binary not found: {}", bin),
            Self::ProcessNotRunning => write!(f, "Subprocess is not running"),
            Self::Protocol(err) => write!(f, "Protocol error: {}", err),
            Self::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<ProtocolError> for EngineError {
    fn from(err: ProtocolError) -> Self {
        Self::Protocol(err.to_string())
    }
}

impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub agy_bin: PathBuf,
    pub opencode_bin: PathBuf,
    pub backend: BackendKind,
    pub conversation_id: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
    pub cwd: Option<PathBuf>,
    pub dangerously_skip_permissions: bool,
    pub extra_args: Vec<String>,
}

impl EngineConfig {
    pub fn new() -> Self {
        let bin = Self::resolve_agy_binary();
        let user_config = crate::infra::config_store::load();
        Self {
            agy_bin: bin,
            opencode_bin: resolve_opencode_binary(),
            backend: user_config.backend,
            conversation_id: None,
            project: None,
            model: Some(user_config.model),
            mode: Some(user_config.mode),
            effort: Some(user_config.effort),
            cwd: None,
            dangerously_skip_permissions: true,
            extra_args: Vec::new(),
        }
    }

    pub fn new_isolated() -> Self {
        let bin = Self::resolve_agy_binary();
        let user_config = crate::config::UserConfig::new_isolated();
        Self {
            agy_bin: bin,
            opencode_bin: resolve_opencode_binary(),
            backend: user_config.backend,
            conversation_id: None,
            project: None,
            model: Some(user_config.model),
            mode: Some(user_config.mode),
            effort: Some(user_config.effort),
            cwd: None,
            dangerously_skip_permissions: true,
            extra_args: Vec::new(),
        }
    }

    pub fn resolve_agy_binary() -> PathBuf {
        if let Ok(path_str) = env::var("AGY_BIN") {
            let candidate = PathBuf::from(path_str);
            if candidate.is_file() {
                return candidate;
            }
        }
        if let Ok(home) = env::var("HOME") {
            let default_local = PathBuf::from(home).join(".local/bin/agy");
            if default_local.is_file() {
                return default_local;
            }
        }
        PathBuf::from("agy")
    }

    pub fn default_fallback_models() -> Vec<(String, String)> {
        vec![
            (
                "gemini-3.8-flash-high".to_string(),
                "Gemini 3.8 Flash (High)".to_string(),
            ),
            (
                "gemini-3.8-flash-medium".to_string(),
                "Gemini 3.8 Flash (Medium)".to_string(),
            ),
            (
                "gemini-3.8-flash-low".to_string(),
                "Gemini 3.8 Flash (Low)".to_string(),
            ),
            (
                "gemini-4-argon-high".to_string(),
                "Gemini 4 Argon (High)".to_string(),
            ),
            (
                "gemini-4-argon-medium".to_string(),
                "Gemini 4 Argon (Medium)".to_string(),
            ),
            (
                "gemini-4-argon-low".to_string(),
                "Gemini 4 Argon (Low)".to_string(),
            ),
            (
                "claude-opus-5-5-high".to_string(),
                "Claude Opus 5.5 (High)".to_string(),
            ),
            (
                "claude-sonnet-5-5-high".to_string(),
                "Claude Sonnet 5.5 (High)".to_string(),
            ),
            (
                "gpt-oss-120b-medium".to_string(),
                "GPT-OSS 120B (Medium)".to_string(),
            ),
        ]
    }

    pub async fn fetch_available_models(agy_bin: &Path) -> Vec<(String, String)> {
        let bin_owned = agy_bin.to_path_buf();
        let Ok(Ok(output)) = tokio::task::spawn_blocking(move || {
            opencode::run_opencode_command_with_bin(&bin_owned, &["models"], Duration::from_secs(3))
        })
        .await
        else {
            return Self::default_fallback_models();
        };

        let text = String::from_utf8_lossy(&output);
        let mut models = Vec::new();
        for line in text.lines() {
            let cleaned = strip_ansi(line);
            let trimmed = cleaned.trim();
            if trimmed.is_empty() || trimmed.starts_with("Fetching") {
                continue;
            }
            let parts: Vec<&str> = trimmed.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let id = parts[0].trim().to_string();
                let name = parts[1].trim().to_string();
                if !id.is_empty() && !id.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    models.push((id, name));
                }
            } else {
                let space_parts: Vec<&str> = trimmed.split_whitespace().collect();
                if !space_parts.is_empty() {
                    let id = space_parts[0].to_string();
                    if id.chars().any(char::is_control) {
                        continue;
                    }
                    let name = if space_parts.len() > 1 {
                        space_parts[1..].join(" ")
                    } else {
                        id.clone()
                    };
                    models.push((id, name));
                }
            }
        }
        if models.is_empty() {
            Self::default_fallback_models()
        } else {
            models
        }
    }

    pub fn split_model_effort(model: &str) -> (&str, Option<&str>) {
        let suffixes = ["-low", "-medium", "-high", "-xhigh", "-max"];
        for sfx in suffixes {
            if let Some(stripped) = model.strip_suffix(sfx) {
                if !stripped.is_empty() {
                    return (stripped, Some(&sfx[1..]));
                }
            }
        }
        (model, None)
    }

    pub fn strip_effort_suffix(model: &str) -> &str {
        Self::split_model_effort(model).0
    }

    pub fn allows_auto_permissions(&self) -> bool {
        self.dangerously_skip_permissions
            && self
                .mode
                .as_deref()
                .and_then(BackendKind::normalize_mode)
                .unwrap_or(crate::config::DEFAULT_MODE)
                == "auto"
    }

    pub fn build_acp_args(&self) -> Vec<String> {
        let mut args = vec!["acp".to_string(), "--stdio".to_string()];
        if self.allows_auto_permissions() {
            args.push("--dangerously-skip-permissions".to_string());
        }
        args.extend(self.extra_args.clone());
        args
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::new()
    }
}
