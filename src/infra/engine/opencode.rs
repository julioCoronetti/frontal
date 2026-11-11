use std::process::Stdio;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::process::{ChildStderr, Command};
use tokio::sync::{mpsc, RwLock};
use tokio_util::codec::{FramedRead, LinesCodec};

use super::{register_child_pgid, unregister_child_pgid};
use crate::protocol::{
    parse_engine_line, EngineEvent, ErrorEvent, InitEvent, ResultData, ResultEvent, ResultStatus,
    SessionEnvelope, SessionId, StreamEvent, TokenUsage,
};
use crate::transport::{build_opencode_args, EngineConfig, EngineError};

pub struct OpenCodeEngine {
    session_id: SessionId,
    config: EngineConfig,
    conversation_id: Arc<RwLock<Option<String>>>,
    active_pgid: Arc<AtomicI32>,
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
            sender,
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

    pub async fn send_prompt(&mut self, prompt: &str) -> Result<(), EngineError> {
        if self.active_pgid.load(Ordering::Relaxed) > 0 {
            return Err(EngineError::Io("turn already running".to_string()));
        }
        let current_conversation = self.conversation_id.read().await.clone();
        let args = build_opencode_args(&self.config, current_conversation.as_deref(), prompt);
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
        self.active_pgid.store(pgid, Ordering::Relaxed);
        register_child_pgid(pgid);

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let sender = self.sender.clone();
        let session_id = self.session_id.clone();
        let conversation_id = Arc::clone(&self.conversation_id);
        let active_pgid = Arc::clone(&self.active_pgid);

        tokio::spawn(async move {
            let stderr_task = tokio::spawn(read_stderr_tail(stderr));
            let mut usage = TokenUsage::default();
            let mut saw_terminal = false;
            if let Some(stream) = stdout {
                let codec = LinesCodec::new_with_max_length(16 * 1024 * 1024);
                let mut reader = FramedRead::new(stream, codec);
                while let Some(line_res) = reader.next().await {
                    let Ok(line) = line_res else {
                        break;
                    };
                    match parse_engine_line(&line) {
                        Ok(Some(mut event)) => {
                            if matches!(event, StreamEvent::Error(_) | StreamEvent::Result(_)) {
                                saw_terminal = true;
                            }
                            if let Some(conv) = event.conversation_id().map(str::to_string) {
                                if !conv.is_empty() {
                                    *conversation_id.write().await = Some(conv);
                                }
                            }
                            if let StreamEvent::StepUpdate(update) = &mut event {
                                if let Some(step) = update.step_update.as_mut() {
                                    if let Some(step_usage) = step.usage {
                                        usage.accumulate(&step_usage);
                                        step.usage = Some(usage);
                                    }
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
            let stderr_tail = stderr_task.await.unwrap_or_default();
            unregister_child_pgid(pgid);
            active_pgid.store(0, Ordering::Relaxed);

            if saw_terminal {
                return;
            }

            let event = match &exit_status {
                Ok(exit) if exit.success() => StreamEvent::Result(ResultEvent {
                    conversation_id: conversation_id.read().await.clone(),
                    result: Some(ResultData {
                        status: Some(ResultStatus::Success),
                        usage: Some(usage),
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
        let pgid = self.active_pgid.load(Ordering::Relaxed);
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
        let pgid = self.active_pgid.load(Ordering::Relaxed);
        if pgid > 0 {
            unsafe {
                libc::killpg(pgid, libc::SIGTERM);
            }
            let deadline = tokio::time::Instant::now() + grace;
            while self.active_pgid.load(Ordering::Relaxed) > 0
                && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            if self.active_pgid.load(Ordering::Relaxed) > 0 {
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                }
            }
            unregister_child_pgid(pgid);
            self.active_pgid.store(0, Ordering::Relaxed);
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
        let pgid = self.active_pgid.load(Ordering::Relaxed);
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
    let mut tail: Vec<String> = Vec::new();
    while let Some(Ok(line)) = reader.next().await {
        if tail.len() >= 64 {
            tail.remove(0);
        }
        tail.push(line);
    }
    tail.join("\n")
}
