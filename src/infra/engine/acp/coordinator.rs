use serde_json::json;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, RwLock};

use super::setup::spawn_stdin_writer;
use super::{AcpCommand, WireEvent};
use crate::protocol::acp::{
    format_method_not_found, format_permission_response, parse_session_update,
    select_permission_option_id, AcpContentBlock, AcpIncoming, AcpNotification, AcpRequest,
    AcpStreamState,
};
use crate::protocol::{
    EngineEvent, ResultData, ResultEvent, ResultStatus, SessionEnvelope, SessionId, StreamEvent,
};
use crate::transport::EngineConfig;

pub(super) enum HandshakePhase {
    InitSent(u64),
    LoadSent(u64),
    NewSent(u64),
    SetupPending(HashSet<u64>),
    Ready,
    Failed,
}

pub(super) struct CoordinatorCtx {
    pub(super) session_id: SessionId,
    pub(super) conversation_id: Arc<RwLock<String>>,
    pub(super) config: EngineConfig,
    pub(super) cwd: String,
    child_pid: Option<u32>,
    stdin_tx: Option<mpsc::UnboundedSender<String>>,
    pub(super) sender: mpsc::Sender<SessionEnvelope>,
    next_id: u64,
    pub(super) phase: HandshakePhase,
    pub(super) acp_session_id: Option<String>,
    pub(super) served_model: Option<String>,
    pub(super) served_mode: Option<String>,
    pub(super) mode_setup_fallback: Option<(u64, String)>,
    pub(super) stream_state: AcpStreamState,
    pub(super) buffered_updates: Vec<StreamEvent>,
    pub(super) active_prompt_id: Option<u64>,
    pub(super) queued_prompts: VecDeque<Vec<AcpContentBlock>>,
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

    pub(super) fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn write_line(&self, line: String) -> bool {
        self.stdin_tx
            .as_ref()
            .is_some_and(|tx| tx.send(line).is_ok())
    }

    pub(super) fn write_request(&self, req: &AcpRequest) -> bool {
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

    pub(super) async fn send_stream_event(&self, ev: StreamEvent) {
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
