use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::engine::SubprocessEngine;
use crate::protocol::{EngineEvent, SessionEnvelope, SessionId, StreamEvent};
use crate::transport::{EngineConfig, EngineError, FixtureReplayEngine, ReplayMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Initializing,
    Ready,
    RunningTurn,
    Interrupted,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMetadata {
    pub session_id: SessionId,
    pub conversation_id: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
    pub turns: u64,
    pub last_prompt: Option<String>,
    pub status: SessionStatus,
}

pub enum SessionEngine {
    Subprocess(Box<SubprocessEngine>),
    Replay(FixtureReplayEngine),
}

pub struct SessionHandle {
    pub metadata: SessionMetadata,
    pub engine: SessionEngine,
}

pub struct SessionManager {
    sessions: HashMap<SessionId, SessionHandle>,
    active_session_id: Option<SessionId>,
    tx: mpsc::Sender<SessionEnvelope>,
    rx: mpsc::Receiver<SessionEnvelope>,
}

impl SessionManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1024);
        Self {
            sessions: HashMap::new(),
            active_session_id: None,
            tx,
            rx,
        }
    }

    pub fn event_sender(&self) -> mpsc::Sender<SessionEnvelope> {
        self.tx.clone()
    }

    pub fn register_mock_session(&mut self, session_id: SessionId) {
        let metadata = SessionMetadata {
            session_id: session_id.clone(),
            conversation_id: None,
            model: Some(crate::config::DEFAULT_MODEL.to_string()),
            mode: Some(crate::config::DEFAULT_MODE.to_string()),
            effort: Some(crate::config::DEFAULT_EFFORT.to_string()),
            turns: 0,
            last_prompt: None,
            status: SessionStatus::Ready,
        };
        let replay = FixtureReplayEngine::from_lines(
            session_id.clone(),
            Vec::new(),
            ReplayMode::TurnGated,
            self.tx.clone(),
        );
        self.sessions.insert(
            session_id.clone(),
            SessionHandle {
                metadata,
                engine: SessionEngine::Replay(replay),
            },
        );
        if self.active_session_id.is_none() {
            self.active_session_id = Some(session_id);
        }
    }

    pub async fn create_subprocess_session(
        &mut self,
        config: EngineConfig,
    ) -> Result<SessionId, EngineError> {
        let session_id = SessionId::new();
        let metadata = SessionMetadata {
            session_id: session_id.clone(),
            conversation_id: config.conversation_id.clone(),
            model: config.model.clone(),
            mode: config.mode.clone(),
            effort: config.effort.clone(),
            turns: 0,
            last_prompt: None,
            status: SessionStatus::Initializing,
        };
        let engine = SubprocessEngine::spawn(session_id.clone(), config, self.tx.clone()).await?;
        self.sessions.insert(
            session_id.clone(),
            SessionHandle {
                metadata,
                engine: SessionEngine::Subprocess(Box::new(engine)),
            },
        );
        if self.active_session_id.is_none() {
            self.active_session_id = Some(session_id.clone());
        }
        Ok(session_id)
    }

    pub async fn create_replay_session(
        &mut self,
        path: &Path,
        mode: ReplayMode,
    ) -> Result<SessionId, EngineError> {
        let session_id = SessionId::new();
        let metadata = SessionMetadata {
            session_id: session_id.clone(),
            conversation_id: None,
            model: Some("replay-mock".to_string()),
            mode: Some("replay".to_string()),
            effort: None,
            turns: 0,
            last_prompt: None,
            status: SessionStatus::Ready,
        };
        let mut replay =
            FixtureReplayEngine::from_file(session_id.clone(), path, mode, self.tx.clone())?;
        replay.start().await?;
        self.sessions.insert(
            session_id.clone(),
            SessionHandle {
                metadata,
                engine: SessionEngine::Replay(replay),
            },
        );
        if self.active_session_id.is_none() {
            self.active_session_id = Some(session_id.clone());
        }
        Ok(session_id)
    }

    pub async fn create_replay_session_from_lines(
        &mut self,
        lines: Vec<String>,
        mode: ReplayMode,
    ) -> Result<SessionId, EngineError> {
        let session_id = SessionId::new();
        let metadata = SessionMetadata {
            session_id: session_id.clone(),
            conversation_id: None,
            model: Some("replay-mock".to_string()),
            mode: Some("replay".to_string()),
            effort: None,
            turns: 0,
            last_prompt: None,
            status: SessionStatus::Ready,
        };
        let mut replay =
            FixtureReplayEngine::from_lines(session_id.clone(), lines, mode, self.tx.clone());
        replay.start().await?;
        self.sessions.insert(
            session_id.clone(),
            SessionHandle {
                metadata,
                engine: SessionEngine::Replay(replay),
            },
        );
        if self.active_session_id.is_none() {
            self.active_session_id = Some(session_id.clone());
        }
        Ok(session_id)
    }

    pub fn active_session_id(&self) -> Option<&SessionId> {
        self.active_session_id.as_ref()
    }

    pub fn set_active_session(&mut self, id: SessionId) -> bool {
        if self.sessions.contains_key(&id) {
            self.active_session_id = Some(id);
            true
        } else {
            false
        }
    }

    pub fn is_replay_session(&self, session_id: &SessionId) -> bool {
        self.sessions
            .get(session_id)
            .map(|h| matches!(h.engine, SessionEngine::Replay(_)))
            .unwrap_or(false)
    }

    pub fn get_engine_config(&self, session_id: &SessionId) -> Option<EngineConfig> {
        self.sessions.get(session_id).and_then(|h| match &h.engine {
            SessionEngine::Subprocess(engine) => Some(engine.config().clone()),
            SessionEngine::Replay(_) => None,
        })
    }

    pub fn get_metadata(&self, session_id: &SessionId) -> Option<&SessionMetadata> {
        self.sessions.get(session_id).map(|h| &h.metadata)
    }

    pub fn set_session_status(&mut self, session_id: &SessionId, status: SessionStatus) {
        if let Some(h) = self.sessions.get_mut(session_id) {
            h.metadata.status = status;
        }
    }

    pub fn set_session_metadata(
        &mut self,
        session_id: &SessionId,
        conv_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) {
        if let Some(h) = self.sessions.get_mut(session_id) {
            if conv_id.is_some() {
                h.metadata.conversation_id = conv_id;
            }
            if model.is_some() {
                h.metadata.model = model;
            }
            if effort.is_some() {
                h.metadata.effort = effort;
            }
            if mode.is_some() {
                h.metadata.mode = mode;
            }
        }
    }

    pub fn set_session_prompt(&mut self, session_id: &SessionId, prompt: &str) {
        if let Some(h) = self.sessions.get_mut(session_id) {
            if h.metadata.last_prompt.is_none() {
                h.metadata.last_prompt = Some(prompt.to_string());
            }
        }
    }

    pub async fn send_prompt(
        &mut self,
        session_id: &SessionId,
        prompt: &str,
    ) -> Result<(), EngineError> {
        let handle = self
            .sessions
            .get_mut(session_id)
            .ok_or(EngineError::ProcessNotRunning)?;
        if handle.metadata.last_prompt.is_none() {
            handle.metadata.last_prompt = Some(prompt.to_string());
        }
        let prev_status = handle.metadata.status;
        handle.metadata.status = SessionStatus::RunningTurn;
        let res = match &mut handle.engine {
            SessionEngine::Subprocess(engine) => engine.send_prompt(prompt).await,
            SessionEngine::Replay(engine) => engine.send_prompt(prompt).await,
        };
        if res.is_err() {
            handle.metadata.status = prev_status;
        }
        res
    }

    pub fn interrupt(&mut self, session_id: &SessionId) -> Result<(), EngineError> {
        let handle = self
            .sessions
            .get_mut(session_id)
            .ok_or(EngineError::ProcessNotRunning)?;
        handle.metadata.status = SessionStatus::Interrupted;
        match &mut handle.engine {
            SessionEngine::Subprocess(engine) => engine.interrupt(),
            SessionEngine::Replay(engine) => engine.interrupt(),
        }
    }

    pub fn interrupt_active(&mut self) -> Result<(), EngineError> {
        if let Some(active_id) = self.active_session_id.clone() {
            self.interrupt(&active_id)
        } else {
            Err(EngineError::ProcessNotRunning)
        }
    }

    pub async fn shutdown_session(
        &mut self,
        session_id: &SessionId,
        grace: Duration,
    ) -> Result<Option<i32>, EngineError> {
        if self.active_session_id.as_ref() == Some(session_id) {
            self.active_session_id = None;
        }
        if let Some(mut handle) = self.sessions.remove(session_id) {
            handle.metadata.status = SessionStatus::Closed;
            match &mut handle.engine {
                SessionEngine::Subprocess(engine) => engine.shutdown(grace).await,
                SessionEngine::Replay(engine) => engine.shutdown(),
            }
        } else {
            Ok(None)
        }
    }

    pub async fn shutdown_all(&mut self, grace: Duration) -> Result<(), EngineError> {
        let session_ids: Vec<SessionId> = self.sessions.keys().cloned().collect();
        for id in session_ids {
            let _ = self.shutdown_session(&id, grace).await;
        }
        Ok(())
    }

    pub async fn recv(&mut self) -> Option<SessionEnvelope> {
        let envelope = self.rx.recv().await?;
        if let Some(handle) = self.sessions.get_mut(&envelope.session_id) {
            match &envelope.event {
                EngineEvent::Stream(ev) => match &**ev {
                    StreamEvent::Init(init_ev) => {
                        if let Some(conv_id) = &init_ev.conversation_id {
                            handle.metadata.conversation_id = Some(conv_id.clone());
                        }
                        handle.metadata.status = SessionStatus::Ready;
                    }
                    StreamEvent::Result(res_ev) => {
                        handle.metadata.turns += 1;
                        if let Some(res) = &res_ev.result {
                            if let Some(conv_id) = &res.conversation_id {
                                handle.metadata.conversation_id = Some(conv_id.clone());
                            }
                        }
                        handle.metadata.status = SessionStatus::Ready;
                    }
                    _ => {}
                },
                EngineEvent::ProcessExited { .. } => {
                    handle.metadata.status = SessionStatus::Closed;
                }
                _ => {}
            }
        }
        Some(envelope)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
