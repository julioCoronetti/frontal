use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;
use tokio::process::{Child, ChildStderr, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tokio_util::codec::{FramedRead, LinesCodec};

use futures_util::StreamExt;

use crate::protocol::{EngineEvent, SessionEnvelope, SessionId};
use crate::transport::EngineError;

static ACTIVE_CHILD_PGIDS: OnceLock<StdMutex<HashSet<i32>>> = OnceLock::new();
static REAPER_TX: OnceLock<StdMutex<Option<mpsc::UnboundedSender<i32>>>> = OnceLock::new();

fn active_pgids() -> &'static StdMutex<HashSet<i32>> {
    ACTIVE_CHILD_PGIDS.get_or_init(|| StdMutex::new(HashSet::new()))
}

fn reaper_slot() -> &'static StdMutex<Option<mpsc::UnboundedSender<i32>>> {
    REAPER_TX.get_or_init(|| StdMutex::new(None))
}

pub fn register_child_pgid(pgid: i32) {
    if pgid > 0 {
        if let Ok(mut set) = active_pgids().lock() {
            set.insert(pgid);
        }
    }
}

pub fn unregister_child_pgid(pgid: i32) {
    if pgid > 0 {
        if let Ok(mut set) = active_pgids().lock() {
            set.remove(&pgid);
        }
    }
}

fn take_registered_pgid(pgid: i32) -> bool {
    if pgid > 0 {
        if let Ok(mut set) = active_pgids().lock() {
            return set.remove(&pgid);
        }
    }
    false
}

pub fn kill_registered_subprocesses() {
    let pgids: Vec<i32> = match active_pgids().lock() {
        Ok(set) => set.iter().copied().filter(|&p| p > 0).collect(),
        Err(_) => return,
    };
    for pgid in pgids {
        unsafe {
            libc::killpg(pgid, libc::SIGTERM);
        }
    }
}

pub fn init_pgid_reaper() {
    let Ok(mut slot) = reaper_slot().lock() else {
        return;
    };
    if slot.as_ref().is_some_and(|tx| !tx.is_closed()) {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<i32>();
    *slot = Some(tx);
    handle.spawn(async move {
        while let Some(pgid) = rx.recv().await {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if take_registered_pgid(pgid) {
                    unsafe {
                        libc::killpg(pgid, libc::SIGKILL);
                    }
                }
            });
        }
    });
}

pub struct ProcessGroupGuard {
    pgid: i32,
    armed: bool,
}

impl ProcessGroupGuard {
    pub fn from_pid(pid: u32) -> Self {
        let pgid = pid as i32;
        if pgid > 0 {
            register_child_pgid(pgid);
            Self { pgid, armed: true }
        } else {
            Self {
                pgid: 0,
                armed: false,
            }
        }
    }

    pub fn pgid(&self) -> i32 {
        self.pgid
    }

    pub fn disarm(&mut self) {
        if self.armed {
            self.armed = false;
            unregister_child_pgid(self.pgid);
        }
    }

    pub fn signal(&self, sig: i32) {
        if self.armed && self.pgid > 0 {
            unsafe {
                libc::killpg(self.pgid, sig);
            }
        }
    }

    pub fn kill_sigkill_and_disarm(&mut self) {
        if self.armed && self.pgid > 0 {
            unsafe {
                libc::killpg(self.pgid, libc::SIGKILL);
            }
        }
        self.disarm();
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if !self.armed || self.pgid <= 0 {
            return;
        }
        self.armed = false;
        unsafe {
            libc::killpg(self.pgid, libc::SIGTERM);
        }
        init_pgid_reaper();
        if let Ok(slot) = reaper_slot().lock() {
            if let Some(tx) = slot.as_ref() {
                if tx.send(self.pgid).is_ok() {
                    return;
                }
            }
        }
        if take_registered_pgid(self.pgid) {
            unsafe {
                libc::killpg(self.pgid, libc::SIGKILL);
            }
        }
    }
}

pub fn spawn_std_isolated(
    cmd: &mut std::process::Command,
) -> std::io::Result<(std::process::Child, ProcessGroupGuard)> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let parent_pid = unsafe { libc::getpid() };
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
    }
    let child = cmd.spawn()?;
    let guard = ProcessGroupGuard::from_pid(child.id());
    Ok((child, guard))
}

pub(crate) fn spawn_tokio_isolated(
    cmd: &mut Command,
) -> std::io::Result<(Child, ProcessGroupGuard)> {
    let parent_pid = unsafe { libc::getpid() };
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
    let child = cmd.spawn()?;
    let guard = ProcessGroupGuard::from_pid(child.id().unwrap_or(0));
    Ok((child, guard))
}

pub(crate) fn spawn_piped_isolated(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<(Child, ProcessGroupGuard), EngineError> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    spawn_tokio_isolated(&mut cmd).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            EngineError::ProcessNotFound(bin.display().to_string())
        } else {
            EngineError::Io(e.to_string())
        }
    })
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
    guard: Option<&mut ProcessGroupGuard>,
    grace: Duration,
    stderr_tail: &Arc<Mutex<VecDeque<String>>>,
    session_id: &SessionId,
    sender: &mpsc::Sender<SessionEnvelope>,
) -> Result<Option<i32>, EngineError> {
    let child_pid = child.id();

    let wait_result = timeout(grace, child.wait()).await;
    let exit_status = match wait_result {
        Ok(Ok(status)) => {
            if let Some(pid) = child_pid {
                unsafe {
                    libc::killpg(pid as libc::pid_t, libc::SIGTERM);
                }
            }
            status.code()
        }
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

    if let Some(g) = guard {
        g.disarm();
    } else if let Some(pid) = child_pid {
        unregister_child_pgid(pid as i32);
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
