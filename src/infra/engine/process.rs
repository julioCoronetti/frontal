use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, ChildStderr, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tokio_util::codec::{FramedRead, LinesCodec};

use futures_util::StreamExt;

use super::{register_child_pgid, unregister_child_pgid};
use crate::protocol::{EngineEvent, SessionEnvelope, SessionId};
use crate::transport::EngineError;

pub(crate) fn spawn_piped_isolated(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<Child, EngineError> {
    let parent_pid = unsafe { libc::getpid() };
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(dir) = cwd {
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

    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            EngineError::ProcessNotFound(bin.display().to_string())
        } else {
            EngineError::Io(e.to_string())
        }
    })?;

    if let Some(pid) = child.id() {
        register_child_pgid(pid as i32);
    }

    Ok(child)
}

pub(crate) fn spawn_stderr_tail_task(
    stderr: ChildStderr,
    session_id: SessionId,
    sender: mpsc::Sender<SessionEnvelope>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
) {
    tokio::spawn(async move {
        let codec = LinesCodec::new_with_max_length(64 * 1024);
        let mut reader = FramedRead::new(stderr, codec);

        while let Some(line_res) = reader.next().await {
            match line_res {
                Ok(line) => {
                    {
                        let mut tail = stderr_tail.lock().await;
                        if tail.len() >= 256 {
                            tail.pop_front();
                        }
                        tail.push_back(line.clone());
                    }
                    let envelope = SessionEnvelope {
                        session_id: session_id.clone(),
                        event: EngineEvent::Stderr(line),
                    };
                    if sender.send(envelope).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub(crate) async fn shutdown_isolated_child(
    child: &mut Child,
    grace: Duration,
    stderr_tail: &Arc<Mutex<VecDeque<String>>>,
    session_id: &SessionId,
    sender: &mpsc::Sender<SessionEnvelope>,
) -> Result<Option<i32>, EngineError> {
    let child_pid = child.id();
    if let Some(pid) = child_pid {
        unregister_child_pgid(pid as i32);
    }

    let wait_result = timeout(grace, child.wait()).await;
    let exit_status = match wait_result {
        Ok(Ok(status)) => status.code(),
        _ => {
            if let Some(pid) = child_pid {
                unsafe {
                    libc::killpg(pid as libc::pid_t, libc::SIGTERM);
                }
            }
            match timeout(Duration::from_millis(1000), child.wait()).await {
                Ok(Ok(status)) => status.code(),
                _ => {
                    if let Some(pid) = child_pid {
                        unsafe {
                            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
                        }
                    }
                    child.wait().await.ok().and_then(|s| s.code())
                }
            }
        }
    };

    if let Some(pid) = child_pid {
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    let tail = {
        let guard = stderr_tail.lock().await;
        guard.iter().cloned().collect()
    };

    let _ = sender
        .send(SessionEnvelope {
            session_id: session_id.clone(),
            event: EngineEvent::ProcessExited {
                code: exit_status,
                stderr_tail: tail,
            },
        })
        .await;

    Ok(exit_status)
}

pub(crate) fn kill_pgid_sigterm(child_pid: Option<u32>) {
    if let Some(pid) = child_pid {
        let pgid = pid as i32;
        unregister_child_pgid(pgid);
        unsafe {
            libc::killpg(pgid, libc::SIGTERM);
        }
    }
}
