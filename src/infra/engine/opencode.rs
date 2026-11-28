use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::process::{ChildStderr, Command};
use tokio::sync::{mpsc, Notify, RwLock};
use tokio_util::codec::{FramedRead, LinesCodec};

use super::process::{spawn_tokio_isolated, ProcessGroupGuard};
use crate::protocol::{
    parse_engine_line, EngineEvent, ErrorEvent, InitEvent, PromptPayload, ProtocolError,
    ResultData, ResultEvent, ResultStatus, SessionEnvelope, SessionId, StreamEvent, TokenUsage,
};
use crate::transport::{build_opencode_payload_args, EngineConfig, EngineError};

enum OpenCodeCmd {
    SendPrompt(PromptPayload),
    Close,
}

pub struct OpenCodeEngine {
    session_id: SessionId,
    config: EngineConfig,
    conversation_id: Arc<RwLock<Option<String>>>,
    active_pgid: Arc<AtomicI32>,
    pgid_guard: Arc<StdMutex<Option<ProcessGroupGuard>>>,
    pgid_notify: Arc<Notify>,
    shutting_down: Arc<AtomicBool>,
    pending_interrupt: Arc<AtomicBool>,
    sender: mpsc::Sender<SessionEnvelope>,
    cmd_tx: mpsc::Sender<OpenCodeCmd>,
}

struct OpenCodeWorker {
    session_id: SessionId,
    config: EngineConfig,
    conversation_id: Arc<RwLock<Option<String>>>,
    active_pgid: Arc<AtomicI32>,
    pgid_guard: Arc<StdMutex<Option<ProcessGroupGuard>>>,
    pgid_notify: Arc<Notify>,
    shutting_down: Arc<AtomicBool>,
    pending_interrupt: Arc<AtomicBool>,
    sender: mpsc::Sender<SessionEnvelope>,
}

impl OpenCodeEngine {
    pub fn new(
        session_id: SessionId,
        config: EngineConfig,
        sender: mpsc::Sender<SessionEnvelope>,
    ) -> Self {
        let initial_conversation = config.conversation_id.clone();
        let init_event = StreamEvent::Init(InitEvent {
            conversation_id: initial_conversation.clone(),
            init: None,
        });
        let _ = sender.try_send(SessionEnvelope {
            session_id: session_id.clone(),
            event: EngineEvent::Stream(Box::new(init_event)),
        });
        let conversation_id = Arc::new(RwLock::new(initial_conversation));
        let active_pgid = Arc::new(AtomicI32::new(0));
        let pgid_guard = Arc::new(StdMutex::new(None));
        let pgid_notify = Arc::new(Notify::new());
        let shutting_down = Arc::new(AtomicBool::new(false));
        let pending_interrupt = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let worker = OpenCodeWorker {
            session_id: session_id.clone(),
            config: config.clone(),
            conversation_id: Arc::clone(&conversation_id),
            active_pgid: Arc::clone(&active_pgid),
            pgid_guard: Arc::clone(&pgid_guard),
            pgid_notify: Arc::clone(&pgid_notify),
            shutting_down: Arc::clone(&shutting_down),
            pending_interrupt: Arc::clone(&pending_interrupt),
            sender: sender.clone(),
        };
        tokio::spawn(run_opencode_worker(worker, cmd_rx));
        Self {
            session_id,
            config,
            conversation_id,
            active_pgid,
            pgid_guard,
            pgid_notify,
            shutting_down,
            pending_interrupt,
            sender,
            cmd_tx,
        }
    }

    async fn wait_for_pgid_clear(&self, timeout: Duration) -> bool {
        wait_for_atomic_pgid_clear(&self.active_pgid, &self.pgid_notify, timeout).await
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub async fn conversation_id(&self) -> Option<String> {
        self.conversation_id.read().await.clone()
    }

    pub fn try_send_prompt(&mut self, prompt: impl Into<PromptPayload>) -> Result<(), EngineError> {
        let payload = prompt.into();
        if payload.is_empty() {
            return Err(EngineError::from(ProtocolError::EmptyPrompt));
        }
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(EngineError::ProcessNotRunning);
        }
        self.pending_interrupt.store(false, Ordering::Release);
        match self.cmd_tx.try_send(OpenCodeCmd::SendPrompt(payload)) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(cmd)) => {
                let tx = self.cmd_tx.clone();
                tokio::spawn(async move {
                    let _ = tx.send(cmd).await;
                });
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(EngineError::ProcessNotRunning),
        }
    }

    pub async fn send_prompt(
        &mut self,
        prompt: impl Into<PromptPayload>,
    ) -> Result<(), EngineError> {
        let payload = prompt.into();
        if payload.is_empty() {
            return Err(EngineError::from(ProtocolError::EmptyPrompt));
        }
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(EngineError::ProcessNotRunning);
        }
        self.pending_interrupt.store(false, Ordering::Release);
        self.cmd_tx
            .send(OpenCodeCmd::SendPrompt(payload))
            .await
            .map_err(|_| EngineError::ProcessNotRunning)
    }

    pub fn interrupt(&mut self) -> Result<(), EngineError> {
        let pgid = self.active_pgid.load(Ordering::SeqCst);
        if pgid > 0 {
            crate::infra::engine::process::signal_pgid(pgid, libc::SIGINT);
            Ok(())
        } else {
            self.pending_interrupt.store(true, Ordering::SeqCst);
            let pgid = self.active_pgid.load(Ordering::SeqCst);
            if pgid > 0 && self.pending_interrupt.swap(false, Ordering::SeqCst) {
                crate::infra::engine::process::signal_pgid(pgid, libc::SIGINT);
                Ok(())
            } else {
                Err(EngineError::ProcessNotRunning)
            }
        }
    }

    pub async fn shutdown(&mut self, grace: Duration) -> Result<Option<i32>, EngineError> {
        self.shutting_down.store(true, Ordering::Release);
        let _ = self.cmd_tx.try_send(OpenCodeCmd::Close);
        let pgid = self.active_pgid.load(Ordering::Acquire);
        if pgid > 0 {
            if let Some(ref g) = *self.pgid_guard.lock().unwrap_or_else(|e| e.into_inner()) {
                g.signal(libc::SIGTERM);
            }
            if !self.wait_for_pgid_clear(grace).await {
                if let Some(mut g) = self
                    .pgid_guard
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                {
                    g.kill_sigkill_and_disarm();
                }
                let _ = self.wait_for_pgid_clear(Duration::from_millis(500)).await;
            }
            if let Some(mut g) = self
                .pgid_guard
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                g.disarm();
            }
            let _ =
                self.active_pgid
                    .compare_exchange(pgid, 0, Ordering::Release, Ordering::Relaxed);
            self.pgid_notify.notify_waiters();
        }
        let _ = self
            .sender
            .send(SessionEnvelope {
                session_id: self.session_id.clone(),
                event: EngineEvent::ProcessExited {
                    code: None,
                    stderr_tail: Vec::new(),
                },
            })
            .await;
        Ok(None)
    }
}

impl Drop for OpenCodeEngine {
    fn drop(&mut self) {
        self.shutting_down.store(true, Ordering::Release);
        self.active_pgid.store(0, Ordering::Release);
        if let Ok(mut g) = self.pgid_guard.lock() {
            drop(g.take());
        }
    }
}

async fn wait_for_atomic_pgid_clear(
    active_pgid: &AtomicI32,
    pgid_notify: &Notify,
    timeout: Duration,
) -> bool {
    if active_pgid.load(Ordering::Acquire) == 0 {
        return true;
    }
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let notified = pgid_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if active_pgid.load(Ordering::Acquire) == 0 {
            return true;
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return active_pgid.load(Ordering::Acquire) == 0;
        }
    }
}

async fn emit_opencode_error(
    sender: &mpsc::Sender<SessionEnvelope>,
    session_id: &SessionId,
    message: String,
) {
    let event = StreamEvent::Error(ErrorEvent {
        error: Some(message),
        message: None,
    });
    let _ = sender
        .send(SessionEnvelope {
            session_id: session_id.clone(),
            event: EngineEvent::Stream(Box::new(event)),
        })
        .await;
}

async fn run_opencode_worker(worker: OpenCodeWorker, mut cmd_rx: mpsc::Receiver<OpenCodeCmd>) {
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            OpenCodeCmd::SendPrompt(payload) => {
                if worker.shutting_down.load(Ordering::Acquire) {
                    break;
                }
                if !wait_for_atomic_pgid_clear(
                    &worker.active_pgid,
                    &worker.pgid_notify,
                    Duration::from_millis(500),
                )
                .await
                {
                    emit_opencode_error(
                        &worker.sender,
                        &worker.session_id,
                        String::from("turn already running"),
                    )
                    .await;
                    continue;
                }
                if worker.shutting_down.load(Ordering::Acquire) {
                    break;
                }
                let current_conversation = worker.conversation_id.read().await.clone();
                let args = build_opencode_payload_args(
                    &worker.config,
                    current_conversation.as_deref(),
                    &payload,
                );
                let mut cmd = Command::new(&worker.config.opencode_bin);
                cmd.args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true);
                if let Some(dir) = &worker.config.cwd {
                    cmd.current_dir(dir);
                }
                let (mut child, guard) = match spawn_tokio_isolated(&mut cmd) {
                    Ok(spawned) => spawned,
                    Err(e) => {
                        let msg = if e.kind() == std::io::ErrorKind::NotFound {
                            EngineError::ProcessNotFound(
                                worker.config.opencode_bin.display().to_string(),
                            )
                            .to_string()
                        } else {
                            EngineError::Io(e.to_string()).to_string()
                        };
                        emit_opencode_error(&worker.sender, &worker.session_id, msg).await;
                        continue;
                    }
                };
                if worker.shutting_down.load(Ordering::Acquire) {
                    let mut guard = guard;
                    guard.signal(libc::SIGTERM);
                    guard.disarm();
                    break;
                }
                let pgid = guard.pgid();
                worker.active_pgid.store(pgid, Ordering::SeqCst);
                if let Ok(mut g) = worker.pgid_guard.lock() {
                    *g = Some(guard);
                }
                if worker.pending_interrupt.swap(false, Ordering::SeqCst) {
                    crate::infra::engine::process::signal_pgid(pgid, libc::SIGINT);
                }
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                let sender = worker.sender.clone();
                let session_id = worker.session_id.clone();
                let conversation_id = Arc::clone(&worker.conversation_id);
                let active_pgid = Arc::clone(&worker.active_pgid);
                let pgid_guard = Arc::clone(&worker.pgid_guard);
                let pgid_notify = Arc::clone(&worker.pgid_notify);
                tokio::spawn(async move {
                    let stderr_task = tokio::spawn(read_stderr_tail(stderr));
                    let mut usage = TokenUsage::default();
                    let mut saw_usage = false;
                    let mut saw_terminal = false;
                    if let Some(stream) = stdout {
                        let codec = LinesCodec::new_with_max_length(16 * 1024 * 1024);
                        let mut reader = FramedRead::new(stream, codec);
                        while let Some(line_res) = reader.next().await {
                            let Ok(line) = line_res else {
                                break;
                            };
                            match parse_engine_line(&line) {
                                Ok(Some(event)) => {
                                    if matches!(
                                        event,
                                        StreamEvent::Error(_) | StreamEvent::Result(_)
                                    ) {
                                        saw_terminal = true;
                                    }
                                    if let Some(conv) = event.conversation_id().map(str::to_string)
                                    {
                                        if !conv.is_empty() {
                                            *conversation_id.write().await = Some(conv);
                                        }
                                    }
                                    if let StreamEvent::StepUpdate(update) = &event {
                                        if let Some(step_usage) =
                                            update.step_update.as_ref().and_then(|step| step.usage)
                                        {
                                            usage.accumulate(&step_usage);
                                            saw_usage = true;
                                        }
                                    }
                                    let envelope = SessionEnvelope {
                                        session_id: session_id.clone(),
                                        event: EngineEvent::Stream(Box::new(event)),
                                    };
                                    if sender.send(envelope).await.is_err() {
                                        break;
                                    }
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    let envelope = SessionEnvelope {
                                        session_id: session_id.clone(),
                                        event: EngineEvent::ProtocolError {
                                            message: err.to_string(),
                                            raw_preview: Some(line),
                                        },
                                    };
                                    let _ = sender.send(envelope).await;
                                }
                            }
                        }
                    }
                    let exit_status = child.wait().await;
                    let taken_guard = {
                        let mut slot = pgid_guard.lock().unwrap_or_else(|e| e.into_inner());
                        if slot.as_ref().is_some_and(|g| g.pgid() == pgid) {
                            slot.take()
                        } else {
                            None
                        }
                    };
                    if let Some(mut guard) = taken_guard {
                        guard.signal(libc::SIGTERM);
                        guard.disarm();
                    }
                    let cleared = active_pgid
                        .compare_exchange(pgid, 0, Ordering::SeqCst, Ordering::Relaxed)
                        .is_ok();
                    pgid_notify.notify_waiters();
                    let stderr_tail = stderr_task.await.unwrap_or_default();
                    if !cleared || saw_terminal {
                        return;
                    }
                    let event = match &exit_status {
                        Ok(exit) if exit.success() => StreamEvent::Result(ResultEvent {
                            conversation_id: conversation_id.read().await.clone(),
                            result: Some(ResultData {
                                status: Some(ResultStatus::Success),
                                usage: saw_usage.then_some(usage),
                                ..Default::default()
                            }),
                        }),
                        Ok(exit) => StreamEvent::Error(ErrorEvent {
                            error: Some(format!(
                                "opencode exited with code {}: {}",
                                exit.code().map_or_else(
                                    || String::from("unknown"),
                                    |code| code.to_string(),
                                ),
                                stderr_tail.trim()
                            )),
                            message: None,
                        }),
                        Err(err) => StreamEvent::Error(ErrorEvent {
                            error: Some(err.to_string()),
                            message: None,
                        }),
                    };
                    let _ = sender
                        .send(SessionEnvelope {
                            session_id,
                            event: EngineEvent::Stream(Box::new(event)),
                        })
                        .await;
                });
            }
            OpenCodeCmd::Close => break,
        }
    }
}

async fn read_stderr_tail(stderr: Option<ChildStderr>) -> String {
    let Some(stream) = stderr else {
        return String::new();
    };
    let codec = LinesCodec::new_with_max_length(64 * 1024);
    let mut reader = FramedRead::new(stream, codec);
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    while let Some(Ok(line)) = reader.next().await {
        if tail.len() >= 64 {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    tail.make_contiguous().join("\n")
}
