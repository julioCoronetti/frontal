use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, RwLock};

use super::setup::{collect_setup_requests, current_mode_fallback, spawn_stdin_writer};
use super::{AcpCommand, WireEvent};
use crate::protocol::acp::{
    format_method_not_found, format_permission_response, parse_prompt_result, parse_session_update,
    select_permission_option_id, AcpContentBlock, AcpIncoming, AcpNotification, AcpRequest,
    AcpRpcError, AcpStreamState,
};
use crate::protocol::{
    EngineEvent, InitData, InitEvent, ResultData, ResultEvent, ResultStatus, SessionEnvelope,
    SessionId, StreamEvent,
};
use crate::transport::EngineConfig;

enum HandshakePhase {
    InitSent(u64),
    LoadSent(u64),
    NewSent(u64),
    SetupPending(HashSet<u64>),
    Ready,
    Failed,
}

pub(super) struct CoordinatorCtx {
    session_id: SessionId,
    conversation_id: Arc<RwLock<String>>,
    config: EngineConfig,
    cwd: String,
    child_pid: Option<u32>,
    stdin_tx: Option<mpsc::UnboundedSender<String>>,
    sender: mpsc::Sender<SessionEnvelope>,
    next_id: u64,
    phase: HandshakePhase,
    acp_session_id: Option<String>,
    served_model: Option<String>,
    served_mode: Option<String>,
    mode_setup_fallback: Option<(u64, String)>,
    stream_state: AcpStreamState,
    buffered_updates: Vec<StreamEvent>,
    active_prompt_id: Option<u64>,
    queued_prompts: VecDeque<Vec<AcpContentBlock>>,
}

impl CoordinatorCtx {
    pub(super) fn new(
        session_id: SessionId,
        conversation_id: Arc<RwLock<String>>,
        config: EngineConfig,
        cwd: String,
        child_pid: Option<u32>,
        stdin: Option<ChildStdin>,
        sender: mpsc::Sender<SessionEnvelope>,
    ) -> Self {
        let served_model = config.model.clone();
        let served_mode = config.mode.clone();
        let stdin_tx = stdin.map(spawn_stdin_writer);
        Self {
            session_id,
            conversation_id,
            config,
            cwd,
            child_pid,
            stdin_tx,
            sender,
            next_id: 1,
            phase: HandshakePhase::InitSent(1),
            acp_session_id: None,
            served_model,
            served_mode,
            mode_setup_fallback: None,
            stream_state: AcpStreamState::default(),
            buffered_updates: Vec::new(),
            active_prompt_id: None,
            queued_prompts: VecDeque::new(),
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn write_line(&self, line: String) -> bool {
        self.stdin_tx
            .as_ref()
            .is_some_and(|tx| tx.send(line).is_ok())
    }

    fn write_request(&self, req: &AcpRequest) -> bool {
        match req.to_ndjson() {
            Ok(line) => self.write_line(line),
            Err(_) => false,
        }
    }

    fn write_notification(&self, notif: &AcpNotification) -> bool {
        match notif.to_ndjson() {
            Ok(line) => self.write_line(line),
            Err(_) => false,
        }
    }

    fn allows_auto_permissions(&self) -> bool {
        self.config.dangerously_skip_permissions
            && self
                .served_mode
                .as_deref()
                .or(self.config.mode.as_deref())
                .and_then(crate::config::BackendKind::normalize_mode)
                .unwrap_or(crate::config::DEFAULT_MODE)
                == "auto"
    }

    async fn send_stream_event(&self, ev: StreamEvent) {
        let _ = self
            .sender
            .send(SessionEnvelope {
                session_id: self.session_id.clone(),
                event: EngineEvent::Stream(Box::new(ev)),
            })
            .await;
    }

    async fn handle_incoming(&mut self, incoming: AcpIncoming) {
        match incoming {
            AcpIncoming::PermissionRequest { id, options, .. } => {
                let opt = if self.allows_auto_permissions() {
                    select_permission_option_id(&options)
                } else {
                    None
                };
                if let Ok(line) = format_permission_response(id, opt.as_deref()) {
                    let _ = self.write_line(line);
                }
            }
            AcpIncoming::UnsupportedRequest { id, method } => {
                if let Ok(line) = format_method_not_found(id, &method) {
                    let _ = self.write_line(line);
                }
            }
            AcpIncoming::Notification { method, params } => {
                if method == "session/update" {
                    if let Some(ev) = parse_session_update(&params, &mut self.stream_state) {
                        match self.phase {
                            HandshakePhase::Ready => {
                                self.send_stream_event(ev).await;
                            }
                            HandshakePhase::SetupPending(_) => {
                                self.buffered_updates.push(ev);
                            }
                            HandshakePhase::InitSent(_)
                            | HandshakePhase::LoadSent(_)
                            | HandshakePhase::NewSent(_)
                            | HandshakePhase::Failed => {}
                        }
                    }
                }
            }
            AcpIncoming::Response { id, result, error } => {
                self.handle_response(id, result, error).await;
            }
            AcpIncoming::Unknown => {}
        }
    }

    async fn handle_response(
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
                if error.is_some() {
                    if let Some((mode_id, fallback)) = self.mode_setup_fallback.take() {
                        if mode_id == id {
                            self.served_mode = Some(fallback);
                        }
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

    async fn fail_handshake(&mut self, message: String) {
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
        let buffered: Vec<_> = self.buffered_updates.drain(..).collect();
        for ev in buffered {
            self.send_stream_event(ev).await;
        }
        self.dispatch_next_prompt();
    }

    fn dispatch_next_prompt(&mut self) {
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

pub(super) async fn run_coordinator(
    mut ctx: CoordinatorCtx,
    mut cmd_rx: mpsc::Receiver<AcpCommand>,
    mut wire_rx: mpsc::Receiver<WireEvent>,
) {
    let init_id = ctx.alloc_id();
    ctx.phase = HandshakePhase::InitSent(init_id);
    let init_req = AcpRequest::new(
        init_id,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false },
                "terminal": false
            },
            "clientInfo": {
                "name": "frontal",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    );
    if !ctx.write_request(&init_req) {
        return;
    }

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    AcpCommand::SendPrompt(blocks) => {
                        if matches!(ctx.phase, HandshakePhase::Failed) {
                            let err_result = StreamEvent::Result(ResultEvent {
                                conversation_id: ctx.acp_session_id.clone(),
                                result: Some(ResultData {
                                    conversation_id: ctx.acp_session_id.clone(),
                                    status: Some(ResultStatus::Error),
                                    error: Some("ACP session failed to initialize".to_string()),
                                    model: ctx.served_model.clone(),
                                    context_window: ctx.stream_state.context_window_hint,
                                    ..Default::default()
                                }),
                            });
                            let _ = ctx.sender.send(SessionEnvelope {
                                session_id: ctx.session_id.clone(),
                                event: EngineEvent::Stream(Box::new(err_result)),
                            }).await;
                        } else {
                            ctx.queued_prompts.push_back(blocks);
                            ctx.dispatch_next_prompt();
                        }
                    }
                    AcpCommand::CancelTurn => {
                        ctx.queued_prompts.clear();
                        if let Some(sid) = ctx.acp_session_id.clone() {
                            let notif = AcpNotification::new("session/cancel", json!({ "sessionId": sid }));
                            let _ = ctx.write_notification(&notif);
                        } else if let Some(pid) = ctx.child_pid {
                            unsafe { libc::killpg(pid as libc::pid_t, libc::SIGINT); }
                        }
                    }
                    AcpCommand::CloseStdin => {
                        ctx.stdin_tx.take();
                        break;
                    }
                }
            }
            Some(wire) = wire_rx.recv() => {
                match wire {
                    WireEvent::Incoming(incoming) => {
                        ctx.handle_incoming(incoming).await;
                    }
                    WireEvent::ProtocolError { message, raw_preview } => {
                        let _ = ctx.sender.send(SessionEnvelope {
                            session_id: ctx.session_id.clone(),
                            event: EngineEvent::ProtocolError { message, raw_preview },
                        }).await;
                    }
                    WireEvent::Eof => break,
                }
            }
            else => break,
        }
    }
}
