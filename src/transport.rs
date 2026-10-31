use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

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
    SlashCommandNotAllowed(String),
    Timeout,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "IO error: {}", err),
            Self::ProcessNotFound(bin) => write!(f, "Binary not found: {}", bin),
            Self::ProcessNotRunning => write!(f, "Subprocess is not running"),
            Self::Protocol(err) => write!(f, "Protocol error: {}", err),
            Self::SlashCommandNotAllowed(cmd) => {
                write!(f, "Slash command '{}' must be handled locally", cmd)
            }
            Self::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<ProtocolError> for EngineError {
    fn from(err: ProtocolError) -> Self {
        match err {
            ProtocolError::SlashCommandNotAllowed(cmd) => Self::SlashCommandNotAllowed(cmd),
            other => Self::Protocol(other.to_string()),
        }
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
        Self {
            agy_bin: bin,
            conversation_id: None,
            project: None,
            model: None,
            mode: None,
            effort: None,
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
            args.push(model.clone());
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
