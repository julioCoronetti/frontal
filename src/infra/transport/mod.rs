use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

mod opencode;

pub use opencode::{
    build_opencode_args, fetch_opencode_models, model_with_variant, opencode_variant,
    parse_opencode_models_output, resolve_opencode_binary,
};

use crate::config::BackendKind;
use crate::protocol::{
    format_user_message, parse_stream_line, EngineEvent, ProtocolError, SessionEnvelope, SessionId,
    StreamEvent,
};

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
        let user_config = crate::config::UserConfig::load();
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
        let output = match tokio::time::timeout(
            Duration::from_secs(3),
            tokio::process::Command::new(agy_bin).arg("models").output(),
        )
        .await
        {
            Ok(Ok(out)) if out.status.success() => out.stdout,
            _ => return Self::default_fallback_models(),
        };

        let text = String::from_utf8_lossy(&output);
        let mut models = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("Fetching") {
                continue;
            }
            let parts: Vec<&str> = trimmed.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let id = parts[0].trim().to_string();
                let name = parts[1].trim().to_string();
                if !id.is_empty() {
                    models.push((id, name));
                }
            } else {
                let space_parts: Vec<&str> = trimmed.split_whitespace().collect();
                if !space_parts.is_empty() {
                    let id = space_parts[0].to_string();
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

    pub fn strip_effort_suffix(model: &str) -> &str {
        let suffixes = ["-low", "-medium", "-high", "-xhigh", "-max"];
        for sfx in suffixes {
            if let Some(stripped) = model.strip_suffix(sfx) {
                return stripped;
            }
        }
        model
    }

    pub fn build_args(&self, resolved_conversation_id: &str) -> Vec<String> {
        let mut args = vec![
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--conversation".to_string(),
            resolved_conversation_id.to_string(),
        ];
        if let Some(project) = &self.project {
            args.push("--project".to_string());
            args.push(project.clone());
        }
        if let Some(model) = &self.model {
            args.push("--model".to_string());
            let final_model = if self.effort.is_some() {
                Self::strip_effort_suffix(model)
            } else {
                model.as_str()
            };
            args.push(final_model.to_string());
        }
        if let Some(mode) = &self.mode {
            args.push("--mode".to_string());
            args.push(mode.clone());
        }
        if let Some(effort) = &self.effort {
            args.push("--effort".to_string());
            args.push(effort.clone());
        }
        if self.dangerously_skip_permissions {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayMode {
    Immediate,
    TurnGated,
}

pub struct FixtureReplayEngine {
    session_id: SessionId,
    lines: Arc<Vec<String>>,
    cursor: Arc<AtomicUsize>,
    mode: ReplayMode,
    sender: mpsc::Sender<SessionEnvelope>,
    interrupted: Arc<AtomicBool>,
}

impl FixtureReplayEngine {
    pub fn from_file(
        session_id: SessionId,
        path: &Path,
        mode: ReplayMode,
        sender: mpsc::Sender<SessionEnvelope>,
    ) -> Result<Self, EngineError> {
        let raw = fs::read_to_string(path)?;
        let lines: Vec<String> = raw
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(Self {
            session_id,
            lines: Arc::new(lines),
            cursor: Arc::new(AtomicUsize::new(0)),
            mode,
            sender,
            interrupted: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn from_lines(
        session_id: SessionId,
        lines: Vec<String>,
        mode: ReplayMode,
        sender: mpsc::Sender<SessionEnvelope>,
    ) -> Self {
        Self {
            session_id,
            lines: Arc::new(lines),
            cursor: Arc::new(AtomicUsize::new(0)),
            mode,
            sender,
            interrupted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn start(&mut self) -> Result<(), EngineError> {
        match self.mode {
            ReplayMode::Immediate => {
                let lines = self.lines.clone();
                let cursor = self.cursor.clone();
                let interrupted = self.interrupted.clone();
                let sender = self.sender.clone();
                let session_id = self.session_id.clone();
                tokio::spawn(async move {
                    while cursor.load(Ordering::Relaxed) < lines.len() {
                        if interrupted.load(Ordering::Relaxed) {
                            break;
                        }
                        let idx = cursor.fetch_add(1, Ordering::Relaxed);
                        if idx >= lines.len() {
                            break;
                        }
                        let line = &lines[idx];
                        if let Ok(Some(stream_ev)) = parse_stream_line(line) {
                            let envelope = SessionEnvelope {
                                session_id: session_id.clone(),
                                event: EngineEvent::Stream(Box::new(stream_ev)),
                            };
                            let _ = sender.send(envelope).await;
                        }
                        sleep(Duration::from_millis(1)).await;
                    }
                });
            }
            ReplayMode::TurnGated => {
                if self.cursor.load(Ordering::Relaxed) < self.lines.len() {
                    let first_line = &self.lines[self.cursor.load(Ordering::Relaxed)];
                    if let Ok(Some(stream_ev)) = parse_stream_line(first_line) {
                        if matches!(stream_ev, StreamEvent::Init(_)) {
                            let envelope = SessionEnvelope {
                                session_id: self.session_id.clone(),
                                event: EngineEvent::Stream(Box::new(stream_ev)),
                            };
                            let _ = self.sender.send(envelope).await;
                            self.cursor.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn send_prompt(&mut self, prompt: &str) -> Result<(), EngineError> {
        let _ = format_user_message(prompt)?;
        self.interrupted.store(false, Ordering::Relaxed);
        let lines = self.lines.clone();
        let cursor = self.cursor.clone();
        let interrupted = self.interrupted.clone();
        let sender = self.sender.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            while cursor.load(Ordering::Relaxed) < lines.len() {
                if interrupted.load(Ordering::Relaxed) {
                    let _ = sender
                        .send(SessionEnvelope {
                            session_id,
                            event: EngineEvent::Stderr("Replay interrupted by user".to_string()),
                        })
                        .await;
                    break;
                }
                let idx = cursor.fetch_add(1, Ordering::Relaxed);
                if idx >= lines.len() {
                    break;
                }
                let line = &lines[idx];
                if let Ok(Some(stream_ev)) = parse_stream_line(line) {
                    let is_result = stream_ev.is_result();
                    let envelope = SessionEnvelope {
                        session_id: session_id.clone(),
                        event: EngineEvent::Stream(Box::new(stream_ev)),
                    };
                    let _ = sender.send(envelope).await;
                    if is_result {
                        break;
                    }
                }
                sleep(Duration::from_millis(2)).await;
            }
        });
        Ok(())
    }

    pub fn interrupt(&mut self) -> Result<(), EngineError> {
        self.interrupted.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<Option<i32>, EngineError> {
        self.interrupted.store(true, Ordering::Relaxed);
        Ok(Some(0))
    }
}
