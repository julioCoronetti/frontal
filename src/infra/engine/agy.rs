use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};

use futures_util::StreamExt;

use super::process::{
    kill_pgid_sigterm, shutdown_isolated_child, spawn_piped_isolated, spawn_stderr_tail_task,
};
use crate::protocol::{
    format_user_payload, parse_stream_line, EngineEvent, PromptPayload, SessionEnvelope, SessionId,
    StreamEvent,
};
use crate::transport::{EngineConfig, EngineError};

pub struct AgyEngine {
    session_id: SessionId,
    conversation_id: Arc<RwLock<String>>,
    config: EngineConfig,
    stdin: Option<ChildStdin>,
    child: Child,
    sender: mpsc::Sender<SessionEnvelope>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl AgyEngine {
    pub async fn spawn(
        session_id: SessionId,
        config: EngineConfig,
        sender: mpsc::Sender<SessionEnvelope>,
    ) -> Result<Self, EngineError> {
        let active_conversation_id = config
            .conversation_id
            .clone()
            .unwrap_or_else(|| session_id.to_string());
        let conversation_id = Arc::new(RwLock::new(active_conversation_id.clone()));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(256)));

        let args = config.build_args(&active_conversation_id);
        let mut child = spawn_piped_isolated(&config.agy_bin, &args, config.cwd.as_deref())?;

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
            spawn_stderr_tail_task(
                stderr_stream,
                session_id.clone(),
                sender.clone(),
                Arc::clone(&stderr_tail),
            );
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

    pub async fn send_prompt(
        &mut self,
        prompt: impl Into<PromptPayload>,
    ) -> Result<(), EngineError> {
        let payload = format_user_payload(&prompt.into())?;
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
        shutdown_isolated_child(
            &mut self.child,
            grace,
            &self.stderr_tail,
            &self.session_id,
            &self.sender,
        )
        .await
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

impl Drop for AgyEngine {
    fn drop(&mut self) {
        kill_pgid_sigterm(self.child.id());
    }
}
