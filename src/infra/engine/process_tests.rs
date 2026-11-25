use super::*;

static TEST_REAPER_LOCK: StdMutex<()> = StdMutex::new(());

struct ReaperTestGuard;

impl Drop for ReaperTestGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = reaper_cell().lock() {
            *guard = None;
        }
        drain_and_sigkill_active_pgids();
        HAS_DRAIN_WAITERS.store(false, Ordering::Release);
    }
}

#[test]
fn clear_reaper_sender_ignores_stale_generation() {
    let _lock = TEST_REAPER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _reset = ReaperTestGuard;
    let (tx, _rx) = std::sync::mpsc::channel();
    *reaper_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some((42, tx));

    clear_reaper_sender(41);
    assert_eq!(existing_reaper_sender().map(|(g, _)| g), Some(42));

    clear_reaper_sender(42);
    assert!(existing_reaper_sender().is_none());
}

#[test]
fn drain_pgid_reaper_clears_disconnected_or_wedged_sender() {
    let _lock = TEST_REAPER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _reset = ReaperTestGuard;

    let (tx_disc, rx_disc) = std::sync::mpsc::channel();
    drop(rx_disc);
    *reaper_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some((100, tx_disc));
    assert!(!drain_pgid_reaper(Duration::from_millis(20)));
    assert!(existing_reaper_sender().is_none());

    let (tx_mid, rx_mid) = std::sync::mpsc::channel();
    *reaper_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some((101, tx_mid));
    let mid_drop = std::thread::spawn(move || {
        let _ = rx_mid.recv();
    });
    assert!(!drain_pgid_reaper(Duration::from_millis(50)));
    let _ = mid_drop.join();
    assert!(existing_reaper_sender().is_none());

    let (tx_hung, _rx_hung) = std::sync::mpsc::channel();
    *reaper_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some((102, tx_hung));
    assert!(!drain_pgid_reaper(Duration::from_millis(10)));
    assert!(existing_reaper_sender().is_none());
}

#[test]
fn drain_pgid_reaper_handles_uninitialized_all_clear_wake_and_escalate() {
    use std::os::unix::process::ExitStatusExt;

    let _lock = TEST_REAPER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _reset = ReaperTestGuard;

    assert!(drain_pgid_reaper(Duration::from_millis(10)));

    let mut cmd = std::process::Command::new("sleep");
    cmd.arg("10");
    let (mut child, guard) = spawn_std_isolated(&mut cmd).expect("spawn isolated child");
    std::mem::forget(guard);
    assert!(!drain_pgid_reaper(Duration::from_millis(10)));
    assert!(active_pgids().lock().is_ok_and(|set| set.is_empty()));
    let status = child.wait().expect("wait killed child");
    assert_eq!(status.signal(), Some(libc::SIGKILL));

    init_pgid_reaper();
    assert!(drain_pgid_reaper(Duration::from_millis(100)));

    let mut cmd_wake = std::process::Command::new("sleep");
    cmd_wake.arg("10");
    let (mut child_wake, mut guard_wake) =
        spawn_std_isolated(&mut cmd_wake).expect("spawn wake child");
    let wait_for_waiters = || {
        let start = Instant::now();
        while !HAS_DRAIN_WAITERS.load(Ordering::Acquire)
            && start.elapsed() < Duration::from_millis(200)
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(HAS_DRAIN_WAITERS.load(Ordering::Acquire));
    };
    let drain_thread = std::thread::spawn(|| drain_pgid_reaper(Duration::from_millis(500)));
    wait_for_waiters();
    guard_wake.kill_sigkill_and_disarm();
    assert!(drain_thread.join().expect("join drain thread"));
    let _ = child_wake.wait();

    let spawn_trapped_sleep = || {
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "trap '' TERM; echo r; exec sleep 10"])
            .stdout(Stdio::piped());
        let (mut child, guard) = spawn_std_isolated(&mut cmd).expect("spawn trapped child");
        let mut ready = [0u8; 1];
        std::io::Read::read_exact(child.stdout.as_mut().expect("piped stdout"), &mut ready)
            .expect("trap ready");
        (child, guard)
    };

    let (mut child_sched, guard_sched) = spawn_trapped_sleep();
    let sched_pgid = guard_sched.pgid();
    drop(guard_sched);
    let drain_sched = std::thread::spawn(|| drain_pgid_reaper(Duration::from_millis(500)));
    wait_for_waiters();
    let wake_start = Instant::now();
    unregister_child_pgid(sched_pgid);
    assert!(drain_sched.join().expect("join scheduled wake drain"));
    assert!(wake_start.elapsed() < Duration::from_millis(250));
    let _ = child_sched.kill();
    let _ = child_sched.wait();

    let (mut child2, guard2) = spawn_trapped_sleep();
    drop(guard2);
    let wait_drain = std::thread::spawn(|| drain_pgid_reaper(Duration::from_millis(500)));
    wait_for_waiters();
    assert!(!drain_pgid_reaper(Duration::from_millis(5)));
    assert!(!wait_drain.join().expect("join concurrent waiter"));
    assert!(existing_reaper_sender().is_some());
    assert!(active_pgids().lock().is_ok_and(|set| set.is_empty()));
    let status2 = child2.wait().expect("wait escalated child2");
    assert_eq!(status2.signal(), Some(libc::SIGKILL));
}
