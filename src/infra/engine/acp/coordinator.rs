use serde_json::json;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, Mutex, RwLock};

use super::super::process::{shutdown_isolated_child, ProcessGroupGuard};
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
    child: Arc<Mutex<Option<Child>>>,
    child_pid: Arc<AtomicU32>,
    pgid_guard: Arc<StdMutex<Option<ProcessGroupGuard>>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    shutting_down: Arc<AtomicBool>,
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        session_id: SessionId,
        conversation_id: Arc<RwLock<String>>,
        config: EngineConfig,
        cwd: String,
        child: Arc<Mutex<Option<Child>>>,
        child_pid: Arc<AtomicU32>,
        pgid_guard: Arc<StdMutex<Option<ProcessGroupGuard>>>,
        stderr_tail: Arc<Mutex<VecDeque<String>>>,
        shutting_down: Arc<AtomicBool>,
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
            child,
            child_pid,
            pgid_guard,
            stderr_tail,
            shutting_down,
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

    async fn emit_error_result(&self, message: &str) {
        let err_result = StreamEvent::Result(ResultEvent {
            conversation_id: self.acp_session_id.clone(),
            result: Some(ResultData {
                conversation_id: self.acp_session_id.clone(),
                status: Some(ResultStatus::Error),
                error: Some(message.to_string()),
                model: self.served_model.clone(),
                context_window: self.stream_state.context_window_hint,
                ..Default::default()
            }),
        });
        self.send_stream_event(err_result).await;
    }

    async fn handle_unexpected_eof(&mut self) {
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        if matches!(
            self.phase,
            HandshakePhase::InitSent(_)
                | HandshakePhase::LoadSent(_)
                | HandshakePhase::NewSent(_)
                | HandshakePhase::SetupPending(_)
        ) {
            self.fail_handshake("ACP process exited before handshake completed".to_string())
                .await;
        } else {
            if self.active_prompt_id.take().is_some() {
                self.emit_error_result("ACP process exited mid-turn").await;
            }
            while self.queued_prompts.pop_front().is_some() {
                self.emit_error_result("ACP process exited mid-turn").await;
            }
            self.phase = HandshakePhase::Failed;
        }

        if let Some(mut child) = self.child.lock().await.take() {
            match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                Ok(Ok(status)) => {
                    self.child_pid.store(0, Ordering::Release);
                    if let Some(mut guard) = self
                        .pgid_guard
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take()
                    {
                        guard.signal(libc::SIGTERM);
                        guard.disarm();
                    }
                    let tail = self.stderr_tail.lock().await.iter().cloned().collect();
                    let _ = self
                        .sender
                        .send(SessionEnvelope {
                            session_id: self.session_id.clone(),
                            event: EngineEvent::ProcessExited {
                                code: status.code(),
                                stderr_tail: tail,
                            },
                        })
                        .await;
                }
                _ => {
                    let mut guard = self
                        .pgid_guard
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take();
                    let _ = shutdown_isolated_child(
                        &mut child,
                        guard.as_mut(),
                        Duration::from_millis(200),
                        &self.stderr_tail,
                        &self.session_id,
                        &self.sender,
                    )
                    .await;
                    self.child_pid.store(0, Ordering::Release);
                }
            }
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
        ctx.handle_unexpected_eof().await;
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
                        } else {
                            let pid = ctx.child_pid.load(Ordering::Acquire);
                            if pid != 0 {
                                unsafe { libc::killpg(pid as libc::pid_t, libc::SIGINT); }
                            }
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
                    WireEvent::Eof => {
                        ctx.handle_unexpected_eof().await;
                        break;
                    }
                }
            }
            else => break,
        }
    }
}
