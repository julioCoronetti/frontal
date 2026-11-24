use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
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
    turn_gen: u64,
    cancelled_up_to: Arc<AtomicU64>,
    turn_lock: Arc<Mutex<()>>,
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
        Ok(Self::from_lines(session_id, lines, mode, sender))
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
            turn_gen: 0,
            cancelled_up_to: Arc::new(AtomicU64::new(0)),
            turn_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn start(&mut self) -> Result<(), EngineError> {
        match self.mode {
            ReplayMode::Immediate => {
                self.turn_gen += 1;
                let my_gen = self.turn_gen;
                let lines = self.lines.clone();
                let cursor = self.cursor.clone();
                let cancelled_up_to = self.cancelled_up_to.clone();
                let sender = self.sender.clone();
                let session_id = self.session_id.clone();
                tokio::spawn(async move {
                    while cursor.load(Ordering::Relaxed) < lines.len() {
                        if cancelled_up_to.load(Ordering::Acquire) >= my_gen {
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
                            if sender.send(envelope).await.is_err() {
                                break;
                            }
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
        self.turn_gen += 1;
        let my_gen = self.turn_gen;
        let cancelled_up_to = self.cancelled_up_to.clone();
        let lines = self.lines.clone();
        let cursor = self.cursor.clone();
        let turn_lock = self.turn_lock.clone();
        let sender = self.sender.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            let _guard = turn_lock.lock().await;
            while cursor.load(Ordering::Relaxed) < lines.len() {
                if cancelled_up_to.load(Ordering::Acquire) >= my_gen {
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
                    if sender.send(envelope).await.is_err() {
                        break;
                    }
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
        self.cancelled_up_to
            .fetch_max(self.turn_gen, Ordering::AcqRel);
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<Option<i32>, EngineError> {
        self.cancelled_up_to
            .fetch_max(self.turn_gen, Ordering::AcqRel);
        Ok(Some(0))
    }
}
