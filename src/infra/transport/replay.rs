use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

use super::EngineError;
use crate::protocol::{
    format_user_payload, parse_stream_line, EngineEvent, PromptPayload, SessionEnvelope, SessionId,
    StreamEvent,
};

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

    pub async fn send_prompt(
        &mut self,
        prompt: impl Into<PromptPayload>,
    ) -> Result<(), EngineError> {
        let payload = prompt.into();
        let _ = format_user_payload(&payload)?;
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
