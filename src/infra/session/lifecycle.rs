use std::time::Duration;

use crate::config::BackendKind;
use crate::protocol::{EngineEvent, PromptPayload, SessionEnvelope, SessionId, StreamEvent};
use crate::transport::EngineError;

use super::{FrontalEngine, SessionManager, SessionStatus};

impl SessionManager {
    pub async fn send_prompt(
        &mut self,
        session_id: &SessionId,
        prompt: impl Into<PromptPayload>,
    ) -> Result<(), EngineError> {
        let payload = prompt.into();
        let handle = self
            .sessions
            .get_mut(session_id)
            .ok_or(EngineError::ProcessNotRunning)?;
        if handle.metadata.last_prompt.is_none() {
            let summary = payload.summary_text();
            if !summary.is_empty() {
                handle.metadata.last_prompt = Some(summary);
            }
        }
        let prev_status = handle.metadata.status;
        handle.metadata.status = SessionStatus::RunningTurn;
        let res = match &mut handle.engine {
            FrontalEngine::Acp(engine) => engine.send_prompt(payload).await,
            FrontalEngine::OpenCode(engine) => engine.send_prompt(payload).await,
            FrontalEngine::Replay(engine) => engine.send_prompt(payload).await,
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
        if handle.metadata.status == SessionStatus::Closed {
            return Err(EngineError::ProcessNotRunning);
        }
        if handle.metadata.status == SessionStatus::Interrupted {
            return Ok(());
        }
        let prev_status = handle.metadata.status;
        handle.metadata.status = SessionStatus::Interrupted;
        let res = match &mut handle.engine {
            FrontalEngine::Acp(engine) => engine.interrupt(),
            FrontalEngine::OpenCode(engine) => engine.interrupt(),
            FrontalEngine::Replay(engine) => engine.interrupt(),
        };
        if res.is_err() {
            handle.metadata.status = prev_status;
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

    pub fn detach_and_shutdown_background(&mut self, session_id: &SessionId, grace: Duration) {
        if self.active_session_id.as_ref() == Some(session_id) {
            self.active_session_id = None;
        }
        self.dormant_metadata.remove(session_id);
        self.closing_tasks.retain(|h| !h.is_finished());
        if let Some(mut handle) = self.sessions.remove(session_id) {
            handle.metadata.status = SessionStatus::Closed;
            let task = tokio::spawn(async move {
                match &mut handle.engine {
                    FrontalEngine::Acp(engine) => {
                        let _ = engine.shutdown(grace).await;
                    }
                    FrontalEngine::OpenCode(engine) => {
                        let _ = engine.shutdown(grace).await;
                    }
                    FrontalEngine::Replay(engine) => {
                        let _ = engine.shutdown();
                    }
                }
            });
            self.closing_tasks.push(task);
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
        self.dormant_metadata.remove(session_id);
        if let Some(mut handle) = self.sessions.remove(session_id) {
            handle.metadata.status = SessionStatus::Closed;
            match &mut handle.engine {
                FrontalEngine::Acp(engine) => engine.shutdown(grace).await,
                FrontalEngine::OpenCode(engine) => engine.shutdown(grace).await,
                FrontalEngine::Replay(engine) => engine.shutdown(),
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
