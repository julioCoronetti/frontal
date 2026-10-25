use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::timeout;
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};

use futures_util::StreamExt;

use crate::protocol::{
    format_user_message, parse_stream_line, EngineEvent, SessionEnvelope, SessionId, StreamEvent,
};
use crate::transport::{EngineConfig, EngineError};

pub struct SubprocessEngine {
    session_id: SessionId,
    conversation_id: Arc<RwLock<String>>,
    config: EngineConfig,
    stdin: Option<ChildStdin>,
    child: Child,
    sender: mpsc::Sender<SessionEnvelope>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl SubprocessEngine {
    pub async fn spawn(
        session_id: SessionId,
        config: EngineConfig,
        sender: mpsc::Sender<SessionEnvelope>,
    ) -> Result<Self, EngineError> {
        let parent_pid = unsafe { libc::getpid() };
        let active_conversation_id = config
            .conversation_id
            .clone()
            .unwrap_or_else(|| session_id.to_string());
        let conversation_id = Arc::new(RwLock::new(active_conversation_id.clone()));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(256)));

        let args = config.build_args(&active_conversation_id);
        let mut cmd = Command::new(&config.agy_bin);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(dir) = &config.cwd {
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
                EngineError::ProcessNotFound(config.agy_bin.display().to_string())
            } else {
                EngineError::Io(e.to_string())
            }
        })?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        if let Some(stdout_stream) = stdout {
            let session_id_clone = session_id.clone();
            let sender_clone = sender.clone();
            let conversation_id_clone = Arc::clone(&conversation_id);

            tokio::spawn(async move {
                let codec = LinesCodec::new_with_max_length(16 * 1024 * 1024);
                let mut reader = FramedRead::new(stdout_stream, codec);

                while let Some(line_res) = reader.next().await {
                    match line_res {
                        Ok(line) => match parse_stream_line(&line) {
                            Ok(Some(stream_ev)) => {
                                if let StreamEvent::Init(init_ev) = &stream_ev {
                                    if let Some(conv_id) = &init_ev.conversation_id {
                                        let mut w = conversation_id_clone.write().await;
                                        *w = conv_id.clone();
                                    }
                                }
                                let envelope = SessionEnvelope {
                                    session_id: session_id_clone.clone(),
                                    event: EngineEvent::Stream(Box::new(stream_ev)),
                                };
                                if sender_clone.send(envelope).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                let envelope = SessionEnvelope {
                                    session_id: session_id_clone.clone(),
                                    event: EngineEvent::ProtocolError {
                                        message: e.to_string(),
                                        raw_preview: Some(line),
                                    },
                                };
                                let _ = sender_clone.send(envelope).await;
                            }
                        },
                        Err(LinesCodecError::MaxLineLengthExceeded) => {
                            let envelope = SessionEnvelope {
                                session_id: session_id_clone.clone(),
                                event: EngineEvent::ProtocolError {
                                    message: "Line length exceeded 16MB limit".to_string(),
                                    raw_preview: None,
                                },
                            };
                            let _ = sender_clone.send(envelope).await;
                        }
                        Err(LinesCodecError::Io(io_err)) => {
                            let envelope = SessionEnvelope {
                                session_id: session_id_clone.clone(),
                                event: EngineEvent::ProtocolError {
                                    message: io_err.to_string(),
                                    raw_preview: None,
                                },
                            };
                            let _ = sender_clone.send(envelope).await;
                            break;
                        }
                    }
                }
            });
        }

        if let Some(stderr_stream) = stderr {
            let session_id_clone = session_id.clone();
            let sender_clone = sender.clone();
            let tail_clone = Arc::clone(&stderr_tail);

            tokio::spawn(async move {
                let codec = LinesCodec::new_with_max_length(64 * 1024);
                let mut reader = FramedRead::new(stderr_stream, codec);

                while let Some(line_res) = reader.next().await {
                    match line_res {
                        Ok(line) => {
                            {
                                let mut tail = tail_clone.lock().await;
                                if tail.len() >= 256 {
                                    tail.pop_front();
                                }
                                tail.push_back(line.clone());
                            }
                            let envelope = SessionEnvelope {
                                session_id: session_id_clone.clone(),
                                event: EngineEvent::Stderr(line),
                            };
                            if sender_clone.send(envelope).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        Ok(Self {
            session_id,
            conversation_id,
            config,
            stdin,
            child,
            sender,
            stderr_tail,
        })
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub async fn conversation_id(&self) -> String {
        self.conversation_id.read().await.clone()
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub async fn send_prompt(&mut self, prompt: &str) -> Result<(), EngineError> {
        let payload = format_user_message(prompt)?;
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.write_all(payload.as_bytes()).await?;
            stdin.flush().await?;
            Ok(())
        } else {
            Err(EngineError::ProcessNotRunning)
        }
    }

    pub fn interrupt(&mut self) -> Result<(), EngineError> {
        if let Some(pid) = self.child.id() {
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGINT);
            }
            Ok(())
        } else {
            Err(EngineError::ProcessNotRunning)
        }
    }

    pub async fn shutdown(&mut self, grace: Duration) -> Result<Option<i32>, EngineError> {
        drop(self.stdin.take());
        let wait_result = timeout(grace, self.child.wait()).await;
        let exit_status = match wait_result {
            Ok(Ok(status)) => status.code(),
            _ => {
                if let Some(pid) = self.child.id() {
                    unsafe {
                        libc::killpg(pid as libc::pid_t, libc::SIGTERM);
                    }
                }
                match timeout(Duration::from_millis(1000), self.child.wait()).await {
                    Ok(Ok(status)) => status.code(),
                    _ => {
                        if let Some(pid) = self.child.id() {
                            unsafe {
                                libc::killpg(pid as libc::pid_t, libc::SIGKILL);
                            }
                        }
                        self.child.wait().await.ok().and_then(|s| s.code())
                    }
                }
            }
        };

        let tail = {
            let guard = self.stderr_tail.lock().await;
            guard.iter().cloned().collect()
        };

        let _ = self
            .sender
            .send(SessionEnvelope {
                session_id: self.session_id.clone(),
                event: EngineEvent::ProcessExited {
                    code: exit_status,
                    stderr_tail: tail,
                },
            })
            .await;

        Ok(exit_status)
    }

    pub async fn reconfigure(&mut self, new_config: EngineConfig) -> Result<(), EngineError> {
        let current_conv_id = self.conversation_id().await;
        let mut final_config = new_config;
        if final_config.conversation_id.is_none() {
            final_config.conversation_id = Some(current_conv_id);
        }
        let _ = self.shutdown(Duration::from_millis(500)).await;
        let new_engine =
            Self::spawn(self.session_id.clone(), final_config, self.sender.clone()).await?;
        *self = new_engine;
        Ok(())
    }
}
