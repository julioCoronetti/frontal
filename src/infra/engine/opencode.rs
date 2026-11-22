use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::process::{ChildStderr, Command};
use tokio::sync::{mpsc, Notify, RwLock};
use tokio_util::codec::{FramedRead, LinesCodec};

use super::{register_child_pgid, unregister_child_pgid};
use crate::protocol::{
    parse_engine_line, EngineEvent, ErrorEvent, InitEvent, PromptPayload, ProtocolError,
    ResultData, ResultEvent, ResultStatus, SessionEnvelope, SessionId, StreamEvent, TokenUsage,
};
use crate::transport::{build_opencode_payload_args, EngineConfig, EngineError};

pub struct OpenCodeEngine {
    session_id: SessionId,
    config: EngineConfig,
    conversation_id: Arc<RwLock<Option<String>>>,
    active_pgid: Arc<AtomicI32>,
    pgid_notify: Arc<Notify>,
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
        Self {
            session_id,
            config,
            conversation_id: Arc::new(RwLock::new(initial_conversation)),
            active_pgid: Arc::new(AtomicI32::new(0)),
            pgid_notify: Arc::new(Notify::new()),
            sender,
        }
    }

    async fn wait_for_pgid_clear(&self, timeout: Duration) -> bool {
        if self.active_pgid.load(Ordering::Acquire) == 0 {
            return true;
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.pgid_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.active_pgid.load(Ordering::Acquire) == 0 {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return self.active_pgid.load(Ordering::Acquire) == 0;
            }
        }
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

    pub async fn send_prompt(
        &mut self,
        prompt: impl Into<PromptPayload>,
    ) -> Result<(), EngineError> {
        let payload = prompt.into();
        if payload.is_empty() {
            return Err(EngineError::from(ProtocolError::EmptyPrompt));
        }
        if !self.wait_for_pgid_clear(Duration::from_millis(500)).await {
            return Err(EngineError::Io("turn already running".to_string()));
        }
        let current_conversation = self.conversation_id.read().await.clone();
        let args =
            build_opencode_payload_args(&self.config, current_conversation.as_deref(), &payload);
        let parent_pid = unsafe { libc::getpid() };
        let mut cmd = Command::new(&self.config.opencode_bin);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(dir) = &self.config.cwd {
            cmd.current_dir(dir);
        }

        cmd.process_group(0);

        unsafe {
            cmd.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                if libc::getppid() != parent_pid {
                    libc::_exit(1);
                }
                Ok(())
            });
        }

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                EngineError::ProcessNotFound(self.config.opencode_bin.display().to_string())
            } else {
                EngineError::Io(e.to_string())
            }
        })?;

        let pgid = child.id().map(|pid| pid as i32).unwrap_or(0);
        self.active_pgid.store(pgid, Ordering::Release);
        register_child_pgid(pgid);

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let sender = self.sender.clone();
        let session_id = self.session_id.clone();
        let conversation_id = Arc::clone(&self.conversation_id);
        let active_pgid = Arc::clone(&self.active_pgid);
        let pgid_notify = Arc::clone(&self.pgid_notify);

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
                            if matches!(event, StreamEvent::Error(_) | StreamEvent::Result(_)) {
                                saw_terminal = true;
                            }
                            if let Some(conv) = event.conversation_id().map(str::to_string) {
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
            unregister_child_pgid(pgid);
            let cleared = active_pgid
                .compare_exchange(pgid, 0, Ordering::Release, Ordering::Relaxed)
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
                        exit.code()
                            .map_or_else(|| String::from("unknown"), |code| code.to_string()),
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

        Ok(())
    }

    pub fn interrupt(&mut self) -> Result<(), EngineError> {
        let pgid = self.active_pgid.load(Ordering::Acquire);
        if pgid > 0 {
            unsafe {
                libc::killpg(pgid, libc::SIGINT);
            }
            Ok(())
        } else {
            Err(EngineError::ProcessNotRunning)
        }
    }

    pub async fn shutdown(&mut self, grace: Duration) -> Result<Option<i32>, EngineError> {
        let pgid = self.active_pgid.load(Ordering::Acquire);
        if pgid > 0 {
            unsafe {
                libc::killpg(pgid, libc::SIGTERM);
            }
            if !self.wait_for_pgid_clear(grace).await {
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                }
                let _ = self.wait_for_pgid_clear(Duration::from_millis(500)).await;
            }
            unregister_child_pgid(pgid);
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
        let pgid = self.active_pgid.load(Ordering::Acquire);
        if pgid > 0 {
            unregister_child_pgid(pgid);
            unsafe {
                libc::killpg(pgid, libc::SIGTERM);
            }
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
