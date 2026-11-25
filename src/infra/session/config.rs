use std::ops::{Deref, DerefMut};

use crate::protocol::SessionId;
use crate::transport::EngineConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Initializing,
    Ready,
    RunningTurn,
    Interrupted,
    Closed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionConfig(pub EngineConfig);

impl SessionConfig {
    pub fn from_engine(config: &EngineConfig) -> Self {
        Self(config.clone())
    }

    pub fn to_engine_config(&self) -> EngineConfig {
        self.0.clone()
    }

    pub fn to_metadata(
        &self,
        session_id: SessionId,
        turns: u64,
        last_prompt: Option<String>,
        status: SessionStatus,
    ) -> SessionMetadata {
        SessionMetadata {
            session_id,
            conversation_id: self.0.conversation_id.clone(),
            model: self.0.model.clone(),
            mode: self.0.mode.clone(),
            effort: self.0.effort.clone(),
            turns,
            last_prompt,
            status,
        }
    }
}

impl Deref for SessionConfig {
    type Target = EngineConfig;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SessionConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
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
