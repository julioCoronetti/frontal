use serde_json::{json, Value};
use std::collections::HashSet;

use super::coordinator::{CoordinatorCtx, HandshakePhase};
use super::setup::{collect_setup_requests, current_mode_fallback};
use crate::protocol::acp::{parse_prompt_result, AcpRequest, AcpRpcError};
use crate::protocol::{
    EngineEvent, InitData, InitEvent, ResultData, ResultEvent, ResultStatus, SessionEnvelope,
    StreamEvent,
};

impl CoordinatorCtx {
    pub(super) async fn handle_response(
        &mut self,
        id: u64,
        result: Option<Value>,
        error: Option<AcpRpcError>,
    ) {
        match &mut self.phase {
            HandshakePhase::InitSent(expected) if *expected == id => {
                if let Some(err) = error {
                    self.fail_handshake(format!("ACP initialize failed: {}", err.message))
                        .await;
                    return;
                }
                if let Some(conv_id) = self
                    .config
                    .conversation_id
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string)
                {
                    let req_id = self.alloc_id();
                    self.phase = HandshakePhase::LoadSent(req_id);
                    let req = AcpRequest::new(
                        req_id,
                        "session/load",
                        json!({
                            "sessionId": conv_id,
                            "cwd": self.cwd,
                            "mcpServers": []
                        }),
                    );
                    let _ = self.write_request(&req);
                } else {
                    self.send_session_new();
                }
            }
            HandshakePhase::LoadSent(expected) if *expected == id => {
                if error.is_some() {
                    self.send_session_new();
                } else {
                    let fallback_id = self
                        .config
                        .conversation_id
                        .clone()
                        .unwrap_or_else(|| self.session_id.to_string());
                    self.finish_session_open(result.as_ref(), Some(fallback_id))
                        .await;
                }
            }
            HandshakePhase::NewSent(expected) if *expected == id => {
                if let Some(err) = error {
                    self.fail_handshake(format!("ACP session/new failed: {}", err.message))
                        .await;
                } else {
                    self.finish_session_open(result.as_ref(), None).await;
                }
            }
            HandshakePhase::SetupPending(pending) if pending.contains(&id) => {
                pending.remove(&id);
                if error.is_some()
                    && self
                        .mode_setup_fallback
                        .as_ref()
                        .is_some_and(|(mode_id, _)| *mode_id == id)
                {
                    if let Some((_, fallback)) = self.mode_setup_fallback.take() {
                        self.served_mode = Some(fallback);
                    }
                }
                if pending.is_empty() {
                    self.mode_setup_fallback = None;
                    self.complete_ready().await;
                }
            }
            HandshakePhase::Ready if self.active_prompt_id == Some(id) => {
                self.active_prompt_id = None;
                let ev = parse_prompt_result(
                    self.acp_session_id.clone(),
                    self.served_model.clone(),
                    self.stream_state.context_window_hint,
                    result.as_ref(),
                    error.as_ref(),
                );
                let _ = self
                    .sender
                    .send(SessionEnvelope {
                        session_id: self.session_id.clone(),
                        event: EngineEvent::Stream(Box::new(ev)),
                    })
                    .await;
                self.dispatch_next_prompt();
            }
            _ => {}
        }
    }

    pub(super) async fn fail_handshake(&mut self, message: String) {
        self.phase = HandshakePhase::Failed;
        self.buffered_updates.clear();
        let _ = self
            .sender
            .send(SessionEnvelope {
                session_id: self.session_id.clone(),
                event: EngineEvent::ProtocolError {
                    message: message.clone(),
                    raw_preview: None,
                },
            })
            .await;
        while self.queued_prompts.pop_front().is_some() {
            let err_result = StreamEvent::Result(ResultEvent {
                conversation_id: self.acp_session_id.clone(),
                result: Some(ResultData {
                    conversation_id: self.acp_session_id.clone(),
                    status: Some(ResultStatus::Error),
                    error: Some(message.clone()),
                    model: self.served_model.clone(),
                    context_window: self.stream_state.context_window_hint,
                    ..Default::default()
                }),
            });
            self.send_stream_event(err_result).await;
        }
    }

    fn send_session_new(&mut self) {
        let req_id = self.alloc_id();
        self.phase = HandshakePhase::NewSent(req_id);
        let req = AcpRequest::new(
            req_id,
            "session/new",
            json!({
                "cwd": self.cwd,
                "mcpServers": []
            }),
        );
        let _ = self.write_request(&req);
    }

    async fn finish_session_open(&mut self, result: Option<&Value>, fallback_id: Option<String>) {
        let sid = result
            .and_then(|r| r.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or(fallback_id)
            .unwrap_or_else(|| self.session_id.to_string());
        self.acp_session_id = Some(sid.clone());
        {
            let mut guard = self.conversation_id.write().await;
            *guard = sid.clone();
        }

        let mode_fallback = result.map(current_mode_fallback);
        let setup_reqs = match result {
            Some(res) => collect_setup_requests(
                &sid,
                res,
                &self.config,
                &mut self.served_model,
                &mut self.served_mode,
            ),
            None => Vec::new(),
        };

        if setup_reqs.is_empty() {
            self.complete_ready().await;
        } else {
            self.mode_setup_fallback = None;
            let mut pending = HashSet::with_capacity(setup_reqs.len());
            for (method, params) in setup_reqs {
                let req_id = self.alloc_id();
                if method == "session/set_config_option"
                    && params.get("configId").and_then(Value::as_str) == Some("mode")
                {
                    if let Some(fb) = mode_fallback.clone() {
                        self.mode_setup_fallback = Some((req_id, fb));
                    }
                }
                pending.insert(req_id);
                let req = AcpRequest::new(req_id, method, params);
                let _ = self.write_request(&req);
            }
            self.phase = HandshakePhase::SetupPending(pending);
        }
    }

    async fn complete_ready(&mut self) {
        self.phase = HandshakePhase::Ready;
        let init_ev = StreamEvent::Init(InitEvent {
            conversation_id: self.acp_session_id.clone(),
            init: Some(InitData {
                cwd: Some(self.cwd.clone()),
                tools: Vec::new(),
                permission_mode: self
                    .served_mode
                    .clone()
                    .or_else(|| self.config.mode.clone()),
                model: self.served_model.clone(),
                context_window: self.stream_state.context_window_hint,
            }),
        });
        self.send_stream_event(init_ev).await;
        let buffered = std::mem::take(&mut self.buffered_updates);
        for ev in buffered {
            self.send_stream_event(ev).await;
        }
        self.dispatch_next_prompt();
    }

    pub(super) fn dispatch_next_prompt(&mut self) {
        if !matches!(self.phase, HandshakePhase::Ready) || self.active_prompt_id.is_some() {
            return;
        }
        let Some(sid) = self.acp_session_id.clone() else {
            return;
        };
        let Some(blocks) = self.queued_prompts.pop_front() else {
            return;
        };
        let req_id = self.alloc_id();
        self.active_prompt_id = Some(req_id);
        let req = AcpRequest::new(
            req_id,
            "session/prompt",
            json!({
                "sessionId": sid,
                "prompt": blocks
            }),
        );
        let _ = self.write_request(&req);
    }
}
