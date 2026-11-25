mod coordinator;
mod handshake;
mod setup;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::process::{Child, ChildStdout};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};

use futures_util::StreamExt;

use super::process::{
    shutdown_isolated_child, spawn_piped_isolated, spawn_stderr_tail_task, ProcessGroupGuard,
};
use crate::protocol::acp::{encode_base64, parse_acp_line, AcpContentBlock, AcpIncoming};
use crate::protocol::{
    media_uri_to_path, PromptPayload, ProtocolError, SessionEnvelope, SessionId,
};
use crate::transport::{EngineConfig, EngineError};

use coordinator::{run_coordinator, CoordinatorCtx};

pub(super) enum AcpCommand {
    SendPrompt(Vec<AcpContentBlock>),
    CancelTurn,
    CloseStdin,
}

pub(super) enum WireEvent {
    Incoming(AcpIncoming),
    ProtocolError {
        message: String,
        raw_preview: Option<String>,
    },
    Eof,
}

pub struct AcpEngine {
    session_id: SessionId,
    conversation_id: Arc<RwLock<String>>,
    config: EngineConfig,
    cmd_tx: mpsc::Sender<AcpCommand>,
    child: Arc<Mutex<Option<Child>>>,
    child_pid: Arc<AtomicU32>,
    pgid_guard: Arc<StdMutex<Option<ProcessGroupGuard>>>,
    shutting_down: Arc<AtomicBool>,
    sender: mpsc::Sender<SessionEnvelope>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl AcpEngine {
    pub async fn spawn(
        session_id: SessionId,
        config: EngineConfig,
        sender: mpsc::Sender<SessionEnvelope>,
    ) -> Result<Self, EngineError> {
        let initial_conv_id = config
            .conversation_id
            .clone()
            .unwrap_or_else(|| session_id.to_string());
        let conversation_id = Arc::new(RwLock::new(initial_conv_id));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(256)));

        let args = config.build_acp_args();
        let (mut child, guard) =
            spawn_piped_isolated(&config.agy_bin, &args, config.cwd.as_deref())?;
        let child_pid = Arc::new(AtomicU32::new(child.id().unwrap_or(0)));
        let pgid_guard = Arc::new(StdMutex::new(Some(guard)));
        let shutting_down = Arc::new(AtomicBool::new(false));

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(Some(child)));

        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (wire_tx, wire_rx) = mpsc::channel(256);

        if let Some(stdout_stream) = stdout {
            spawn_stdout_reader(stdout_stream, wire_tx);
        }

        if let Some(stderr_stream) = stderr {
            spawn_stderr_tail_task(
                stderr_stream,
                session_id.clone(),
                sender.clone(),
                Arc::clone(&stderr_tail),
            );
        }

        let cwd = resolve_cwd_string(config.cwd.as_deref());
        let ctx = CoordinatorCtx::new(
            session_id.clone(),
            Arc::clone(&conversation_id),
            config.clone(),
            cwd,
            Arc::clone(&child),
            Arc::clone(&child_pid),
            Arc::clone(&pgid_guard),
            Arc::clone(&stderr_tail),
            Arc::clone(&shutting_down),
            stdin,
            sender.clone(),
        );

        tokio::spawn(run_coordinator(ctx, cmd_rx, wire_rx));

        Ok(Self {
            session_id,
            conversation_id,
            config,
            cmd_tx,
            child,
            child_pid,
            pgid_guard,
            shutting_down,
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
        let payload = prompt.into();
        if payload.is_empty() {
            return Err(ProtocolError::EmptyPrompt.into());
        }
        let mut blocks = Vec::with_capacity(1 + payload.media.len());
        if !payload.text.trim().is_empty() {
            blocks.push(AcpContentBlock::Text { text: payload.text });
        }
        for item in payload.media {
            let path = media_uri_to_path(&item.uri);
            let bytes = tokio::fs::read(&path).await?;
            blocks.push(AcpContentBlock::Image {
                data: encode_base64(&bytes),
                mime_type: item.mime_type,
                uri: None,
            });
        }
        self.cmd_tx
            .send(AcpCommand::SendPrompt(blocks))
            .await
            .map_err(|_| EngineError::ProcessNotRunning)
    }

    pub fn interrupt(&mut self) -> Result<(), EngineError> {
        if self.cmd_tx.try_send(AcpCommand::CancelTurn).is_ok() {
            return Ok(());
        }
        let guard = self.pgid_guard.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref g) = *guard {
            g.signal(libc::SIGINT);
            Ok(())
        } else {
            Err(EngineError::ProcessNotRunning)
        }
    }

    pub async fn shutdown(&mut self, grace: Duration) -> Result<Option<i32>, EngineError> {
        self.shutdown_with_sender(grace, None).await
    }

    async fn shutdown_with_sender(
        &mut self,
        grace: Duration,
        override_sender: Option<&mpsc::Sender<SessionEnvelope>>,
    ) -> Result<Option<i32>, EngineError> {
        self.shutting_down.store(true, Ordering::Release);
        let _ = self.cmd_tx.send(AcpCommand::CloseStdin).await;
        let maybe_child = self.child.lock().await.take();
        if let Some(mut child) = maybe_child {
            let target_sender = override_sender.unwrap_or(&self.sender);
            let mut guard = self.pgid_guard.lock().ok().and_then(|mut g| g.take());
            let res = shutdown_isolated_child(
                &mut child,
                guard.as_mut(),
                grace,
                &self.stderr_tail,
                &self.session_id,
                target_sender,
            )
            .await;
            self.child_pid.store(0, Ordering::Release);
            res
        } else {
            Ok(None)
        }
    }

    pub async fn reconfigure(&mut self, new_config: EngineConfig) -> Result<(), EngineError> {
        let current_conv_id = self.conversation_id().await;
        let mut final_config = new_config;
        if final_config.conversation_id.is_none() {
            final_config.conversation_id = Some(current_conv_id);
        }
        let new_engine =
            match Self::spawn(self.session_id.clone(), final_config, self.sender.clone()).await {
                Ok(engine) => engine,
                Err(err) => {
                    if self.cmd_tx.is_closed() {
                        let _ = self.shutdown(Duration::from_millis(200)).await;
                    }
                    return Err(err);
                }
            };
        self.shutting_down.store(true, Ordering::Release);
        let mut old_engine = std::mem::replace(self, new_engine);
        let (discard_tx, _discard_rx) = mpsc::channel(1);
        let _ = old_engine
            .shutdown_with_sender(Duration::from_millis(500), Some(&discard_tx))
            .await;
        Ok(())
    }
}

impl Drop for AcpEngine {
    fn drop(&mut self) {
        self.shutting_down.store(true, Ordering::Release);
        self.child_pid.store(0, Ordering::Release);
        if let Ok(mut g) = self.pgid_guard.lock() {
            drop(g.take());
        }
    }
}

fn resolve_cwd_string(cwd: Option<&Path>) -> String {
    cwd.map(|p| p.to_path_buf())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .display()
        .to_string()
}

fn spawn_stdout_reader(stdout: ChildStdout, wire_tx: mpsc::Sender<WireEvent>) {
    tokio::spawn(async move {
        let codec = LinesCodec::new_with_max_length(16 * 1024 * 1024);
        let mut reader = FramedRead::new(stdout, codec);

        while let Some(line_res) = reader.next().await {
            let ev = match line_res {
                Ok(line) => match parse_acp_line(&line) {
                    Ok(Some(incoming)) => WireEvent::Incoming(incoming),
                    Ok(None) => continue,
                    Err(e) => WireEvent::ProtocolError {
                        message: e.to_string(),
                        raw_preview: Some(line),
                    },
                },
                Err(LinesCodecError::MaxLineLengthExceeded) => WireEvent::ProtocolError {
                    message: "Line length exceeded 16MB limit".to_string(),
                    raw_preview: None,
                },
                Err(LinesCodecError::Io(io_err)) => {
                    let _ = wire_tx
                        .send(WireEvent::ProtocolError {
                            message: io_err.to_string(),
                            raw_preview: None,
                        })
                        .await;
                    break;
                }
            };
            if wire_tx.send(ev).await.is_err() {
                break;
            }
        }
        let _ = wire_tx.send(WireEvent::Eof).await;
    });
}
