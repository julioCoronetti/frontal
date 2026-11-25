mod config;
mod lifecycle;

use std::collections::HashMap;
use std::path::Path;
use tokio::sync::mpsc;

pub use config::{SessionConfig, SessionMetadata, SessionStatus};

use crate::config::BackendKind;
use crate::engine::{AcpEngine, OpenCodeEngine};
use crate::protocol::{SessionEnvelope, SessionId};
use crate::transport::{EngineConfig, EngineError, FixtureReplayEngine, ReplayMode};

pub enum FrontalEngine {
    Acp(Box<AcpEngine>),
    OpenCode(Box<OpenCodeEngine>),
    Replay(FixtureReplayEngine),
}

pub struct SessionHandle {
    pub config: SessionConfig,
    pub metadata: SessionMetadata,
    engine: Option<FrontalEngine>,
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        let Some(mut engine) = self.engine.take() else {
            return;
        };
        match &mut engine {
            FrontalEngine::Acp(e) => {
                let _ = e.interrupt();
            }
            FrontalEngine::OpenCode(e) => {
                let _ = e.interrupt();
            }
            FrontalEngine::Replay(e) => {
                let _ = e.shutdown();
                return;
            }
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let grace = std::time::Duration::from_millis(250);
                match &mut engine {
                    FrontalEngine::Acp(e) => {
                        let _ = e.shutdown(grace).await;
                    }
                    FrontalEngine::OpenCode(e) => {
                        let _ = e.shutdown(grace).await;
                    }
                    FrontalEngine::Replay(_) => {}
                }
            });
        }
    }
}

pub struct SessionManager {
    pub(super) sessions: HashMap<SessionId, SessionHandle>,
    pub(super) dormant_metadata: HashMap<SessionId, (SessionMetadata, SessionConfig)>,
    pub(super) closing_tasks: Vec<tokio::task::JoinHandle<()>>,
    pub(super) active_session_id: Option<SessionId>,
    tx: mpsc::Sender<SessionEnvelope>,
    rx: mpsc::Receiver<SessionEnvelope>,
}

impl SessionManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1024);
        Self {
            sessions: HashMap::new(),
            dormant_metadata: HashMap::new(),
            closing_tasks: Vec::new(),
            active_session_id: None,
            tx,
            rx,
        }
    }

    pub fn event_sender(&self) -> mpsc::Sender<SessionEnvelope> {
        self.tx.clone()
    }

    pub fn register_mock_session(&mut self, session_id: SessionId) {
        self.register_mock_session_with_metadata(
            session_id,
            None,
            Some(crate::config::DEFAULT_MODEL.to_string()),
            Some(crate::config::DEFAULT_EFFORT.to_string()),
            Some(crate::config::DEFAULT_MODE.to_string()),
        );
    }

    pub fn register_mock_session_with_metadata(
        &mut self,
        session_id: SessionId,
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) {
        let config = SessionConfig(EngineConfig {
            conversation_id,
            model,
            mode,
            effort,
            ..EngineConfig::default()
        });
        let metadata = config.to_metadata(session_id.clone(), 0, None, SessionStatus::Ready);
        let replay = FixtureReplayEngine::from_lines(
            session_id.clone(),
            Vec::new(),
            ReplayMode::TurnGated,
            self.tx.clone(),
        );
        self.insert_session_handle(
            session_id,
            SessionHandle {
                config,
                metadata,
                engine: Some(FrontalEngine::Replay(replay)),
            },
            false,
        );
    }

    pub async fn spawn_engine_detached(
        config: EngineConfig,
        tx: mpsc::Sender<SessionEnvelope>,
    ) -> Result<(SessionId, SessionHandle), EngineError> {
        let session_id = SessionId::new();
        let session_config = SessionConfig::from_engine(&config);
        let metadata =
            session_config.to_metadata(session_id.clone(), 0, None, SessionStatus::Initializing);
        let engine = match config.backend {
            BackendKind::Agy => FrontalEngine::Acp(Box::new(
                AcpEngine::spawn(session_id.clone(), config, tx).await?,
            )),
            BackendKind::OpenCode => FrontalEngine::OpenCode(Box::new(OpenCodeEngine::new(
                session_id.clone(),
                config,
                tx,
            ))),
        };
        Ok((
            session_id,
            SessionHandle {
                config: session_config,
                metadata,
                engine: Some(engine),
            },
        ))
    }

    fn insert_session_handle(
        &mut self,
        session_id: SessionId,
        handle: SessionHandle,
        make_active: bool,
    ) -> SessionId {
        self.dormant_metadata.remove(&session_id);
        if let Some(old_handle) = self.sessions.insert(session_id.clone(), handle) {
            self.push_closing_handle(old_handle, std::time::Duration::from_millis(500));
        }
        if make_active || self.active_session_id.is_none() {
            self.active_session_id = Some(session_id.clone());
        }
        session_id
    }

    pub fn attach_engine(
        &mut self,
        session_id: SessionId,
        handle: SessionHandle,
        make_active: bool,
    ) -> SessionId {
        self.insert_session_handle(session_id, handle, make_active)
    }

    pub async fn create_subprocess_session(
        &mut self,
        config: EngineConfig,
    ) -> Result<SessionId, EngineError> {
        let (session_id, handle) = Self::spawn_engine_detached(config, self.tx.clone()).await?;
        Ok(self.attach_engine(session_id, handle, false))
    }

    async fn insert_replay_engine(
        &mut self,
        session_id: SessionId,
        mut replay: FixtureReplayEngine,
    ) -> Result<SessionId, EngineError> {
        let config = SessionConfig(EngineConfig {
            model: Some("replay-mock".to_string()),
            mode: Some("replay".to_string()),
            ..EngineConfig::default()
        });
        let metadata = config.to_metadata(session_id.clone(), 0, None, SessionStatus::Ready);
        replay.start().await?;
        let sid = self.insert_session_handle(
            session_id,
            SessionHandle {
                config,
                metadata,
                engine: Some(FrontalEngine::Replay(replay)),
            },
            false,
        );
        Ok(sid)
    }

    pub async fn create_replay_session(
        &mut self,
        path: &Path,
        mode: ReplayMode,
    ) -> Result<SessionId, EngineError> {
        let session_id = SessionId::new();
        let replay =
            FixtureReplayEngine::from_file(session_id.clone(), path, mode, self.tx.clone())?;
        self.insert_replay_engine(session_id, replay).await
    }

    pub async fn create_replay_session_from_lines(
        &mut self,
        lines: Vec<String>,
        mode: ReplayMode,
    ) -> Result<SessionId, EngineError> {
        let session_id = SessionId::new();
        let replay =
            FixtureReplayEngine::from_lines(session_id.clone(), lines, mode, self.tx.clone());
        self.insert_replay_engine(session_id, replay).await
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
            .is_some_and(|h| matches!(h.engine.as_ref(), Some(FrontalEngine::Replay(_))))
    }

    pub fn get_session_config(&self, session_id: &SessionId) -> Option<SessionConfig> {
        self.sessions
            .get(session_id)
            .map(|h| h.config.clone())
            .or_else(|| {
                self.dormant_metadata
                    .get(session_id)
                    .map(|(_, cfg)| cfg.clone())
            })
    }

    pub fn get_engine_config(&self, session_id: &SessionId) -> Option<EngineConfig> {
        self.sessions
            .get(session_id)
            .and_then(|h| match h.engine.as_ref() {
                Some(FrontalEngine::Replay(_)) | None => None,
                Some(_) => Some(h.config.to_engine_config()),
            })
            .or_else(|| {
                self.dormant_metadata
                    .get(session_id)
                    .map(|(_, cfg)| cfg.to_engine_config())
            })
    }

    pub fn get_metadata(&self, session_id: &SessionId) -> Option<&SessionMetadata> {
        self.sessions
            .get(session_id)
            .map(|h| &h.metadata)
            .or_else(|| self.dormant_metadata.get(session_id).map(|(m, _)| m))
    }

    #[must_use]
    pub fn save_dormant_metadata(
        &mut self,
        session_id: SessionId,
        meta: SessionMetadata,
        config: EngineConfig,
    ) -> bool {
        if self.sessions.contains_key(&session_id) {
            return false;
        }
        let session_cfg = SessionConfig::from_engine(&config);
        let projected = session_cfg.to_metadata(
            session_id.clone(),
            meta.turns,
            meta.last_prompt,
            meta.status,
        );
        self.dormant_metadata
            .insert(session_id, (projected, session_cfg));
        true
    }

    pub fn remove_dormant_metadata(&mut self, session_id: &SessionId) {
        self.dormant_metadata.remove(session_id);
    }

    pub fn is_dormant(&self, session_id: &SessionId) -> bool {
        self.dormant_metadata.contains_key(session_id)
    }

    fn metadata_and_cfg_mut(
        &mut self,
        session_id: &SessionId,
    ) -> Option<(&mut SessionMetadata, &mut SessionConfig)> {
        if let Some(h) = self.sessions.get_mut(session_id) {
            Some((&mut h.metadata, &mut h.config))
        } else {
            self.dormant_metadata
                .get_mut(session_id)
                .map(|(m, c)| (m, c))
        }
    }

    pub fn set_session_status(&mut self, session_id: &SessionId, status: SessionStatus) {
        if let Some((meta, _)) = self.metadata_and_cfg_mut(session_id) {
            meta.status = status;
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
        if let Some((m, c)) = self.metadata_and_cfg_mut(session_id) {
            if let Some(v) = conv_id {
                c.conversation_id = Some(v);
            }
            if let Some(v) = model {
                c.model = Some(v);
            }
            if let Some(v) = effort {
                c.effort = Some(v);
            }
            if let Some(v) = mode {
                c.mode = Some(v);
            }
            *m = c.to_metadata(
                m.session_id.clone(),
                m.turns,
                m.last_prompt.clone(),
                m.status,
            );
        }
    }

    pub fn set_session_prompt(&mut self, session_id: &SessionId, prompt: &str) {
        if let Some((meta, _)) = self.metadata_and_cfg_mut(session_id) {
            if meta.last_prompt.is_none() {
                meta.last_prompt = Some(prompt.to_string());
            }
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
