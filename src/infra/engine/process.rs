use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::{Child, ChildStderr, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tokio_util::codec::{FramedRead, LinesCodec};

use futures_util::StreamExt;

use crate::protocol::{EngineEvent, SessionEnvelope, SessionId};
use crate::transport::EngineError;

enum ReaperCmd {
    Schedule {
        pgid: i32,
        deadline: Instant,
    },
    Drain {
        done: std::sync::mpsc::SyncSender<bool>,
    },
    EscalatePending(std::sync::mpsc::SyncSender<()>),
    Wake,
}

type ReaperEntry = (u64, std::sync::mpsc::Sender<ReaperCmd>);
type ReaperCell = StdMutex<Option<ReaperEntry>>;

static ACTIVE_CHILD_PGIDS: OnceLock<StdMutex<HashSet<i32>>> = OnceLock::new();
static REAPER_TX: OnceLock<ReaperCell> = OnceLock::new();
static NEXT_REAPER_GEN: AtomicU64 = AtomicU64::new(1);
static HAS_DRAIN_WAITERS: AtomicBool = AtomicBool::new(false);

fn active_pgids() -> &'static StdMutex<HashSet<i32>> {
    ACTIVE_CHILD_PGIDS.get_or_init(|| StdMutex::new(HashSet::new()))
}

fn reaper_cell() -> &'static ReaperCell {
    REAPER_TX.get_or_init(|| StdMutex::new(None))
}

fn existing_reaper_sender() -> Option<ReaperEntry> {
    REAPER_TX
        .get()
        .and_then(|cell| cell.lock().ok())
        .and_then(|guard| guard.clone())
}

fn clear_reaper_sender(gen_id: u64) {
    if let Some(cell) = REAPER_TX.get() {
        if let Ok(mut guard) = cell.lock() {
            if matches!(guard.as_ref(), Some((cur, _)) if *cur == gen_id) {
                *guard = None;
            }
        }
    }
}

fn reaper_sender() -> Option<ReaperEntry> {
    let mut guard = reaper_cell().lock().ok()?;
    if let Some(entry) = guard.as_ref() {
        return Some(entry.clone());
    }
    let (tx, rx) = std::sync::mpsc::channel::<ReaperCmd>();
    let gen_id = NEXT_REAPER_GEN.fetch_add(1, Ordering::Relaxed);
    match std::thread::Builder::new()
        .name(String::from("frontal-pgid-reaper"))
        .spawn(move || run_pgid_reaper(rx))
    {
        Ok(_) => {
            *guard = Some((gen_id, tx.clone()));
            Some((gen_id, tx))
        }
        Err(_) => None,
    }
}

fn reaper_drained(queue: &mut BinaryHeap<Reverse<(Instant, i32)>>) -> bool {
    active_pgids().lock().is_ok_and(|set| {
        queue.retain(|Reverse((_, pgid))| set.contains(pgid));
        queue.is_empty() && set.is_empty()
    })
}

fn run_pgid_reaper(rx: std::sync::mpsc::Receiver<ReaperCmd>) {
    let mut queue: BinaryHeap<Reverse<(Instant, i32)>> = BinaryHeap::new();
    let mut drain_waiters: Vec<std::sync::mpsc::SyncSender<bool>> = Vec::new();
    loop {
        let now = Instant::now();
        while let Some(&Reverse((deadline, _))) = queue.peek() {
            if deadline > now {
                break;
            }
            if let Some(Reverse((_, pgid))) = queue.pop() {
                if take_registered_pgid(pgid) {
                    unsafe {
                        libc::killpg(pgid, libc::SIGKILL);
                    }
                }
            }
        }
        if !drain_waiters.is_empty() && reaper_drained(&mut queue) {
            HAS_DRAIN_WAITERS.store(false, Ordering::Release);
            for waiter in drain_waiters.drain(..) {
                let _ = waiter.send(true);
            }
        }
        let msg = if let Some(&Reverse((deadline, _))) = queue.peek() {
            let wait = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(cmd) => Ok(cmd),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            rx.recv().map_err(|_| ())
        };
        match msg {
            Ok(ReaperCmd::Schedule { pgid, deadline }) => {
                queue.push(Reverse((deadline, pgid)));
            }
            Ok(ReaperCmd::Drain { done }) => {
                if reaper_drained(&mut queue) {
                    let _ = done.send(true);
                } else {
                    drain_waiters.push(done);
                    HAS_DRAIN_WAITERS.store(true, Ordering::Release);
                }
            }
            Ok(ReaperCmd::EscalatePending(ack)) => {
                queue.clear();
                drain_and_sigkill_active_pgids();
                HAS_DRAIN_WAITERS.store(false, Ordering::Release);
                for waiter in drain_waiters.drain(..) {
                    let _ = waiter.send(false);
                }
                let _ = ack.send(());
            }
            Ok(ReaperCmd::Wake) => {}
            Err(()) => break,
        }
    }
    HAS_DRAIN_WAITERS.store(false, Ordering::Release);
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
        let became_empty = if let Ok(mut set) = active_pgids().lock() {
            set.remove(&pgid) && set.is_empty()
        } else {
            false
        };
        if became_empty && HAS_DRAIN_WAITERS.load(Ordering::Acquire) {
            if let Some((_, tx)) = existing_reaper_sender() {
                let _ = tx.send(ReaperCmd::Wake);
            }
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

fn signal_pgids(pgids: Vec<i32>, sig: i32) {
    for pgid in pgids {
        if pgid > 0 {
            unsafe {
                libc::killpg(pgid, sig);
            }
        }
    }
}

pub fn kill_registered_subprocesses() {
    let pgids = active_pgids()
        .lock()
        .ok()
        .map(|set| set.iter().copied().collect());
    if let Some(pgids) = pgids {
        signal_pgids(pgids, libc::SIGTERM);
    }
}

fn drain_and_sigkill_active_pgids() {
    if let Ok(mut set) = active_pgids().lock() {
        let pgids: Vec<i32> = set.drain().collect();
        drop(set);
        signal_pgids(pgids, libc::SIGKILL);
    }
}

pub fn init_pgid_reaper() {
    let _ = reaper_sender();
}

pub fn drain_pgid_reaper(wait: Duration) -> bool {
    let Some((gen_id, tx)) = existing_reaper_sender() else {
        let Ok(pgids) = active_pgids()
            .lock()
            .map(|mut set| set.drain().collect::<Vec<i32>>())
        else {
            return false;
        };
        if pgids.is_empty() {
            return true;
        }
        signal_pgids(pgids, libc::SIGKILL);
        return false;
    };
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    if tx.send(ReaperCmd::Drain { done: done_tx }).is_err() {
        clear_reaper_sender(gen_id);
        drain_and_sigkill_active_pgids();
        return false;
    }
    match done_rx.recv_timeout(wait) {
        Ok(drained) => drained,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
            let escalated_ok = tx.send(ReaperCmd::EscalatePending(ack_tx)).is_ok()
                && ack_rx.recv_timeout(Duration::from_millis(200)).is_ok();
            if !escalated_ok {
                clear_reaper_sender(gen_id);
                drain_and_sigkill_active_pgids();
            }
            false
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            clear_reaper_sender(gen_id);
            drain_and_sigkill_active_pgids();
            false
        }
    }
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
        }
        Self {
            pgid: pgid.max(0),
            armed: pgid > 0,
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
        if self.armed {
            signal_pgid(self.pgid, sig);
        }
    }

    pub fn kill_sigkill_and_disarm(&mut self) {
        if self.armed {
            signal_pgid(self.pgid, libc::SIGKILL);
        }
        self.disarm();
    }
}

pub fn signal_pgid(pgid: i32, sig: i32) {
    if pgid > 0 {
        unsafe {
            libc::killpg(pgid, sig);
        }
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if !self.armed || self.pgid <= 0 {
            return;
        }
        self.armed = false;
        signal_pgid(self.pgid, libc::SIGTERM);
        let deadline = Instant::now() + Duration::from_millis(250);
        if let Some((gen_id, tx)) = reaper_sender() {
            if tx
                .send(ReaperCmd::Schedule {
                    pgid: self.pgid,
                    deadline,
                })
                .is_ok()
            {
                return;
            }
            clear_reaper_sender(gen_id);
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

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
