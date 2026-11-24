use std::path::PathBuf;

use crate::config::BackendKind;
use crate::protocol::SessionId;
use crate::transport::{resolve_opencode_binary, EngineConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Initializing,
    Ready,
    RunningTurn,
    Interrupted,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
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

impl SessionConfig {
    pub fn from_engine(config: &EngineConfig) -> Self {
        Self {
            agy_bin: config.agy_bin.clone(),
            opencode_bin: config.opencode_bin.clone(),
            backend: config.backend,
            conversation_id: config.conversation_id.clone(),
            project: config.project.clone(),
            model: config.model.clone(),
            mode: config.mode.clone(),
            effort: config.effort.clone(),
            cwd: config.cwd.clone(),
            dangerously_skip_permissions: config.dangerously_skip_permissions,
            extra_args: config.extra_args.clone(),
        }
    }

    pub fn to_engine_config(&self) -> EngineConfig {
        EngineConfig {
            agy_bin: self.agy_bin.clone(),
            opencode_bin: self.opencode_bin.clone(),
            backend: self.backend,
            conversation_id: self.conversation_id.clone(),
            project: self.project.clone(),
            model: self.model.clone(),
            mode: self.mode.clone(),
            effort: self.effort.clone(),
            cwd: self.cwd.clone(),
            dangerously_skip_permissions: self.dangerously_skip_permissions,
            extra_args: self.extra_args.clone(),
        }
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
            conversation_id: self.conversation_id.clone(),
            model: self.model.clone(),
            mode: self.mode.clone(),
            effort: self.effort.clone(),
            turns,
            last_prompt,
            status,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            agy_bin: EngineConfig::resolve_agy_binary(),
            opencode_bin: resolve_opencode_binary(),
            backend: BackendKind::Agy,
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
}

impl From<&EngineConfig> for SessionConfig {
    fn from(config: &EngineConfig) -> Self {
        Self::from_engine(config)
    }
}

impl From<&SessionConfig> for EngineConfig {
    fn from(config: &SessionConfig) -> Self {
        config.to_engine_config()
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
