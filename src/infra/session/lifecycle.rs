use std::time::Duration;

use crate::config::BackendKind;
use crate::protocol::{EngineEvent, PromptPayload, SessionEnvelope, SessionId, StreamEvent};
use crate::transport::EngineError;

use super::{FrontalEngine, SessionHandle, SessionManager, SessionStatus};

fn prepare_prompt_target<'a>(
    sessions: &'a mut std::collections::HashMap<SessionId, SessionHandle>,
    session_id: &SessionId,
    payload: &PromptPayload,
) -> Result<(&'a mut SessionHandle, Option<String>), EngineError> {
    let handle = sessions
        .get_mut(session_id)
        .ok_or(EngineError::ProcessNotRunning)?;
    if handle.metadata.status == SessionStatus::Closed {
        return Err(EngineError::ProcessNotRunning);
    }
    let pending_prompt = if handle.metadata.last_prompt.is_none() {
        let summary = payload.summary_text();
        (!summary.is_empty()).then_some(summary)
    } else {
        None
    };
    Ok((handle, pending_prompt))
}

fn commit_prompt_success(handle: &mut SessionHandle, pending_prompt: Option<String>) {
    handle.metadata.status = SessionStatus::RunningTurn;
    if handle.metadata.last_prompt.is_none() {
        if let Some(summary) = pending_prompt {
            handle.metadata.last_prompt = Some(summary);
        }
    }
}

impl SessionManager {
    pub fn dispatch_prompt_sync(
        &mut self,
        session_id: &SessionId,
        prompt: impl Into<PromptPayload>,
    ) -> Result<(), EngineError> {
        let payload = prompt.into();
        let (handle, pending_prompt) =
            prepare_prompt_target(&mut self.sessions, session_id, &payload)?;
        let engine = handle
            .engine
            .as_mut()
            .ok_or(EngineError::ProcessNotRunning)?;
        let res = match engine {
            FrontalEngine::Acp(engine) => engine.try_send_prompt(payload),
            FrontalEngine::OpenCode(engine) => engine.try_send_prompt(payload),
            FrontalEngine::Replay(engine) => engine.try_send_prompt(payload),
        };
        if res.is_ok() {
            commit_prompt_success(handle, pending_prompt);
        }
        res
    }

    pub async fn send_prompt(
        &mut self,
        session_id: &SessionId,
        prompt: impl Into<PromptPayload>,
    ) -> Result<(), EngineError> {
        let payload = prompt.into();
        let (handle, pending_prompt) =
            prepare_prompt_target(&mut self.sessions, session_id, &payload)?;
        let engine = handle
            .engine
            .as_mut()
            .ok_or(EngineError::ProcessNotRunning)?;
        let res = match engine {
            FrontalEngine::Acp(engine) => engine.send_prompt(payload).await,
            FrontalEngine::OpenCode(engine) => engine.send_prompt(payload).await,
            FrontalEngine::Replay(engine) => engine.send_prompt(payload).await,
        };
        if res.is_ok() {
            commit_prompt_success(handle, pending_prompt);
        }
        res
    }

    pub fn interrupt(&mut self, session_id: &SessionId) -> Result<(), EngineError> {
        let handle = self
            .sessions
            .get_mut(session_id)
            .ok_or(EngineError::ProcessNotRunning)?;
        if handle.metadata.status == SessionStatus::Closed {
            return Err(EngineError::ProcessNotRunning);
        }
        if handle.metadata.status == SessionStatus::Interrupted {
            return Ok(());
        }
        let Some(engine) = handle.engine.as_mut() else {
            return Err(EngineError::ProcessNotRunning);
        };
        let res = match engine {
            FrontalEngine::Acp(engine) => engine.interrupt(),
            FrontalEngine::OpenCode(engine) => engine.interrupt(),
            FrontalEngine::Replay(engine) => engine.interrupt(),
        };
        if res.is_ok() {
            handle.metadata.status = SessionStatus::Interrupted;
        }
        res
    }

    pub fn interrupt_active(&mut self) -> Result<(), EngineError> {
        if let Some(active_id) = self.active_session_id.clone() {
            self.interrupt(&active_id)
        } else {
            Err(EngineError::ProcessNotRunning)
        }
    }

    pub(crate) fn detach_engine(&mut self, session_id: &SessionId) -> Option<SessionHandle> {
        if self.active_session_id.as_ref() == Some(session_id) {
            self.active_session_id = None;
        }
        self.dormant_metadata.remove(session_id);
        self.sessions.remove(session_id).map(|mut handle| {
            handle.metadata.status = SessionStatus::Closed;
            handle
        })
    }

    pub(crate) fn push_closing_handle(&mut self, mut handle: SessionHandle, grace: Duration) {
        self.closing_tasks.retain(|h| !h.is_finished());
        if handle.engine.is_none() {
            handle.metadata.status = SessionStatus::Closed;
            return;
        }
        if matches!(handle.engine.as_ref(), Some(FrontalEngine::Replay(_))) {
            if let Some(FrontalEngine::Replay(mut replay)) = handle.engine.take() {
                handle.metadata.status = SessionStatus::Closed;
                let _ = replay.shutdown();
            }
            return;
        }
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            let task = rt.spawn(async move {
                let _ = Self::shutdown_handle(handle, grace).await;
            });
            self.closing_tasks.push(task);
        } else {
            drop(handle);
        }
    }

    pub async fn shutdown_handle(
        mut handle: SessionHandle,
        grace: Duration,
    ) -> Result<Option<i32>, EngineError> {
        handle.metadata.status = SessionStatus::Closed;
        let Some(mut engine) = handle.engine.take() else {
            return Ok(None);
        };
        match &mut engine {
            FrontalEngine::Acp(engine) => engine.shutdown(grace).await,
            FrontalEngine::OpenCode(engine) => engine.shutdown(grace).await,
            FrontalEngine::Replay(engine) => engine.shutdown(),
        }
    }

    pub fn detach_and_shutdown_background(&mut self, session_id: &SessionId, grace: Duration) {
        if let Some(handle) = self.detach_engine(session_id) {
            self.push_closing_handle(handle, grace);
        }
    }

    pub async fn shutdown_session(
        &mut self,
        session_id: &SessionId,
        grace: Duration,
    ) -> Result<Option<i32>, EngineError> {
        if let Some(handle) = self.detach_engine(session_id) {
            Self::shutdown_handle(handle, grace).await
        } else {
            Ok(None)
        }
    }

    pub async fn shutdown_all(&mut self, grace: Duration) -> Result<(), EngineError> {
        let session_ids: Vec<SessionId> = self.sessions.keys().cloned().collect();
        for id in session_ids {
            let _ = self.shutdown_session(&id, grace).await;
        }
        for task in self.closing_tasks.drain(..) {
            let _ = task.await;
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
                            handle.config.conversation_id = Some(conv_id.clone());
                        }
                        if let Some(effective_mode) = init_ev
                            .init
                            .as_ref()
                            .and_then(|init| init.permission_mode.as_deref())
                            .and_then(BackendKind::normalize_mode)
                        {
                            handle.config.mode = Some(effective_mode.to_string());
                        }
                        handle.metadata = handle.config.to_metadata(
                            handle.metadata.session_id.clone(),
                            handle.metadata.turns,
                            handle.metadata.last_prompt.clone(),
                            SessionStatus::Ready,
                        );
                    }
                    StreamEvent::Result(res_ev) => {
                        let turns = handle.metadata.turns + 1;
                        if let Some(res) = &res_ev.result {
                            if let Some(conv_id) = &res.conversation_id {
                                handle.config.conversation_id = Some(conv_id.clone());
                            }
                        }
                        handle.metadata = handle.config.to_metadata(
                            handle.metadata.session_id.clone(),
                            turns,
                            handle.metadata.last_prompt.clone(),
                            SessionStatus::Ready,
                        );
                    }
                    StreamEvent::Error(_) => {
                        handle.metadata.status = SessionStatus::Ready;
                    }
                    StreamEvent::StepUpdate(update_ev) => {
                        let step_cid = update_ev
                            .step_update
                            .as_ref()
                            .and_then(|u| u.conversation_id.as_deref());
                        if let Some(conv_id) = update_ev
                            .conversation_id
                            .as_deref()
                            .or(step_cid)
                            .filter(|id| !id.is_empty())
                        {
                            handle.config.conversation_id = Some(conv_id.to_string());
                            handle.metadata.conversation_id = Some(conv_id.to_string());
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::session::SessionConfig;
    use crate::transport::{EngineConfig, FixtureReplayEngine, ReplayMode};

    fn make_handle(sid: &SessionId, engine: Option<FrontalEngine>) -> SessionHandle {
        let config = SessionConfig::from_engine(&EngineConfig::default());
        let metadata = config.to_metadata(sid.clone(), 0, None, SessionStatus::Ready);
        SessionHandle {
            config,
            metadata,
            engine,
        }
    }

    #[tokio::test]
    async fn save_dormant_is_ignored_when_active_and_push_closing_skips_none() {
        let mut manager = SessionManager::new();
        let sid = SessionId::new();
        let h1 = make_handle(
            &sid,
            Some(FrontalEngine::Replay(FixtureReplayEngine::from_lines(
                sid.clone(),
                Vec::new(),
                ReplayMode::Immediate,
                manager.tx.clone(),
            ))),
        );
        manager.attach_engine(sid.clone(), h1, true);
        let active_meta = manager.get_metadata(&sid).expect("active metadata");
        assert!(!manager.save_dormant_metadata(
            sid.clone(),
            active_meta.clone(),
            EngineConfig::default()
        ));
        assert!(!manager.is_dormant(&sid));

        let h2 = make_handle(&sid, None);
        manager.attach_engine(sid.clone(), h2, false);
        assert!(manager.closing_tasks.is_empty());

        let detached = manager.detach_engine(&sid).expect("detached");
        manager.push_closing_handle(detached, Duration::from_millis(50));
        assert!(manager.closing_tasks.is_empty());
    }

    #[tokio::test]
    async fn attach_engine_collision_clears_dormant_and_errors_roll_back_ready() {
        let mut manager = SessionManager::new();
        let sid = SessionId::new();
        let meta = make_handle(&sid, None).metadata.clone();
        assert!(manager.save_dormant_metadata(sid.clone(), meta, EngineConfig::default()));
        assert!(manager.is_dormant(&sid));

        let h_none = make_handle(&sid, None);
        manager.attach_engine(sid.clone(), h_none, true);
        assert!(!manager.is_dormant(&sid));

        assert!(matches!(
            manager.interrupt(&sid),
            Err(EngineError::ProcessNotRunning)
        ));
        assert_eq!(
            manager.get_metadata(&sid).map(|m| m.status),
            Some(SessionStatus::Ready)
        );
        assert!(matches!(
            manager.dispatch_prompt_sync(&sid, "hi"),
            Err(EngineError::ProcessNotRunning)
        ));
        assert_eq!(
            manager.get_metadata(&sid).map(|m| m.status),
            Some(SessionStatus::Ready)
        );
        assert_eq!(
            manager
                .get_metadata(&sid)
                .and_then(|m| m.last_prompt.clone()),
            None
        );
        assert!(matches!(
            manager.send_prompt(&sid, "hi").await,
            Err(EngineError::ProcessNotRunning)
        ));
        assert_eq!(
            manager.get_metadata(&sid).map(|m| m.status),
            Some(SessionStatus::Ready)
        );
        assert_eq!(
            manager
                .get_metadata(&sid)
                .and_then(|m| m.last_prompt.clone()),
            None
        );

        let replay_handle = make_handle(
            &sid,
            Some(FrontalEngine::Replay(FixtureReplayEngine::from_lines(
                sid.clone(),
                Vec::new(),
                ReplayMode::Immediate,
                manager.tx.clone(),
            ))),
        );
        manager.attach_engine(sid.clone(), replay_handle, false);
        assert!(manager.dispatch_prompt_sync(&sid, "").is_err());
        assert_eq!(
            manager.get_metadata(&sid).map(|m| m.status),
            Some(SessionStatus::Ready)
        );
        assert!(manager.send_prompt(&sid, "").await.is_err());
        assert_eq!(
            manager.get_metadata(&sid).map(|m| m.status),
            Some(SessionStatus::Ready)
        );
        assert_eq!(
            manager
                .get_metadata(&sid)
                .and_then(|m| m.last_prompt.clone()),
            None
        );
    }
}
