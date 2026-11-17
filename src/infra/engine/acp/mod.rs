mod coordinator;
mod setup;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, ChildStdout};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};

use futures_util::StreamExt;

use super::process::{
    kill_pgid_sigterm, shutdown_isolated_child, spawn_piped_isolated, spawn_stderr_tail_task,
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
    child: Child,
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
        let mut child = spawn_piped_isolated(&config.agy_bin, &args, config.cwd.as_deref())?;
        let child_pid = child.id();

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

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
            child_pid,
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
        let _ = self.cmd_tx.send(AcpCommand::CloseStdin).await;
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

impl Drop for AcpEngine {
    fn drop(&mut self) {
        kill_pgid_sigterm(self.child.id());
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
