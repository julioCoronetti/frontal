use std::time::Duration;
use tokio::sync::mpsc;

use frontal::engine::{
    kill_registered_subprocesses, register_child_pgid, unregister_child_pgid, AcpEngine,
};
use frontal::protocol::{EngineEvent, SessionId, StreamEvent};
use frontal::session::{SessionManager, SessionStatus};
use frontal::transport::{EngineConfig, FixtureReplayEngine, ReplayMode};

use crate::common::{TempFileGuard, PGID_TEST_LOCK};

#[tokio::test]
async fn test_fixture_replay_engine_immediate_and_turn_gated() {
    let fixtures = vec![
        r#"{"event":"init","conversation_id":"c1","init":{"tools":["read_file"]}}"#.to_string(),
        r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"answer 1"}}"#.to_string(),
        r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","response":"answer 1"}}"#.to_string(),
        r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":2,"state":"DONE","step_type":"agent_response","text_delta":"answer 2"}}"#.to_string(),
        r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","response":"answer 2"}}"#.to_string(),
    ];

    let session_id = SessionId::new();
    let (tx, mut rx) = mpsc::channel(100);
    let mut replay =
        FixtureReplayEngine::from_lines(session_id.clone(), fixtures, ReplayMode::TurnGated, tx);

    replay.start().await.unwrap();
    let init_env = rx.recv().await.unwrap();
    assert_eq!(init_env.session_id, session_id);
    assert!(matches!(
        *match init_env.event {
            EngineEvent::Stream(s) => s,
            _ => panic!(),
        },
        StreamEvent::Init(_)
    ));

    replay.send_prompt("first prompt").await.unwrap();
    let step1 = rx.recv().await.unwrap();
    assert_eq!(
        match step1.event {
            EngineEvent::Stream(s) => s.text_delta().map(|t| t.to_string()),
            _ => None,
        },
        Some("answer 1".to_string())
    );
    let res1 = rx.recv().await.unwrap();
    assert!(matches!(
        *match res1.event {
            EngineEvent::Stream(s) => s,
            _ => panic!(),
        },
        StreamEvent::Result(_)
    ));

    replay.send_prompt("second prompt").await.unwrap();
    let step2 = rx.recv().await.unwrap();
    assert_eq!(
        match step2.event {
            EngineEvent::Stream(s) => s.text_delta().map(|t| t.to_string()),
            _ => None,
        },
        Some("answer 2".to_string())
    );
    let res2 = rx.recv().await.unwrap();
    assert!(matches!(
        *match res2.event {
            EngineEvent::Stream(s) => s,
            _ => panic!(),
        },
        StreamEvent::Result(_)
    ));
}

#[tokio::test]
async fn test_fixture_replay_engine_cancellation() {
    let lines = [
        r#"{"event":"init","conversation_id":"cancel-conv"}"#,
        r#"{"event":"step_update","step_update":{"step_index":1,"type":"thought","thought":"thinking..."}}"#,
        r#"{"event":"result","result":{"status":"success"}}"#,
    ];
    let fixture_guard = TempFileGuard::new("fixture_cancel", "jsonl", &lines.join("\n"));

    let session_id = SessionId::new();
    let (tx, mut rx) = mpsc::channel(100);

    let mut engine =
        FixtureReplayEngine::from_file(session_id, fixture_guard.path(), ReplayMode::TurnGated, tx)
            .unwrap();

    engine.start().await.unwrap();
    let init_env = rx.recv().await.unwrap();
    match init_env.event {
        EngineEvent::Stream(s) => match *s {
            StreamEvent::Init(ev) => {
                assert_eq!(ev.conversation_id.as_deref(), Some("cancel-conv"));
            }
            other => panic!("Expected Init event, got: {:?}", other),
        },
        other => panic!("Expected Stream event, got: {:?}", other),
    }

    engine.interrupt().unwrap();
    let _ = engine.shutdown();

    let timeout_res = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(timeout_res.is_err() || timeout_res.unwrap().is_none());
}

#[tokio::test]
async fn test_session_manager_multiple_sessions_multiplexing() {
    let mut manager = SessionManager::new();

    let lines1 = vec![
        r#"{"event":"init","conversation_id":"conv-1"}"#.to_string(),
        r#"{"event":"result","result":{"conversation_id":"conv-1","status":"SUCCESS","response":"res1"}}"#.to_string(),
    ];
    let lines2 = vec![
        r#"{"event":"init","conversation_id":"conv-2"}"#.to_string(),
        r#"{"event":"result","result":{"conversation_id":"conv-2","status":"SUCCESS","response":"res2"}}"#.to_string(),
    ];

    let s1 = manager
        .create_replay_session_from_lines(lines1, ReplayMode::TurnGated)
        .await
        .unwrap();
    let s2 = manager
        .create_replay_session_from_lines(lines2, ReplayMode::TurnGated)
        .await
        .unwrap();

    assert_ne!(s1, s2);
    assert_eq!(manager.active_session_id(), Some(&s1));

    manager.set_active_session(s2.clone());
    assert_eq!(manager.active_session_id(), Some(&s2));

    manager.send_prompt(&s1, "prompt 1").await.unwrap();
    manager.send_prompt(&s2, "prompt 2").await.unwrap();

    let mut received_s1 = false;
    let mut received_s2 = false;

    for _ in 0..4 {
        if let Some(env) = manager.recv().await {
            if env.session_id == s1 {
                received_s1 = true;
            }
            if env.session_id == s2 {
                received_s2 = true;
            }
        }
    }

    assert!(received_s1);
    assert!(received_s2);

    let meta1 = manager.get_metadata(&s1).unwrap();
    assert_eq!(meta1.status, SessionStatus::Ready);
    assert_eq!(meta1.turns, 1);

    let meta2 = manager.get_metadata(&s2).unwrap();
    assert_eq!(meta2.status, SessionStatus::Ready);
    assert_eq!(meta2.turns, 1);
}

#[tokio::test]
async fn test_acp_grandchild_process_group_cleanup() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let pid_guard = TempFileGuard::new("grandchild_pid", "txt", "");
    let script = format!(
        "#!/usr/bin/env bash\nsleep 30 >/dev/null 2>&1 &\necho $! > {}\nwhile IFS= read -r _; do :; done\n",
        pid_guard.path().display()
    );
    let script_guard = TempFileGuard::new("grandchild_test", "sh", &script);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(script_guard.path())
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(script_guard.path(), perms).unwrap();
    }

    let session_id = SessionId::new();
    let (tx, _rx) = mpsc::channel(100);
    let mut config = EngineConfig::new_isolated();
    config.agy_bin = script_guard.path().to_path_buf();

    let mut engine = AcpEngine::spawn(session_id, config, tx).await.unwrap();

    let mut grandchild_pid = None;
    for _ in 0..50 {
        if let Ok(pid_str) = std::fs::read_to_string(pid_guard.path()) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                grandchild_pid = Some(pid);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let grandchild_pid = grandchild_pid.expect("grandchild pid should be recorded");

    unsafe {
        assert_eq!(libc::kill(grandchild_pid, 0), 0);
    }

    let _ = engine.shutdown(Duration::from_millis(200)).await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    unsafe {
        let res = libc::kill(grandchild_pid, 0);
        assert_eq!(res, -1);
    }
}

#[test]
fn test_child_pgid_registry_and_kill_safeguard() {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let _lock = PGID_TEST_LOCK.blocking_lock();
    let mut child = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("Failed to spawn sleep");
    let pid = child.id() as i32;

    register_child_pgid(pid);
    unregister_child_pgid(pid);
    kill_registered_subprocesses();

    unsafe {
        assert_eq!(
            libc::kill(pid, 0),
            0,
            "Unregistered child should not be killed by kill_registered_subprocesses"
        );
        let _ = libc::kill(pid, libc::SIGKILL);
    }
    let _ = child.wait();
}

#[test]
fn test_kill_registered_subprocesses_real_termination() {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let _lock = PGID_TEST_LOCK.blocking_lock();
    let mut child = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("Failed to spawn sleep");
    let pid = child.id() as i32;

    register_child_pgid(pid);
    kill_registered_subprocesses();

    let _ = child.wait();
    unsafe {
        let res = libc::kill(pid, 0);
        assert_eq!(res, -1);
    }
}

#[tokio::test]
async fn test_session_manager_get_engine_config() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    assert_eq!(manager.get_engine_config(&sid), None);
}

#[tokio::test]
async fn test_detach_and_shutdown_background_drained_by_shutdown_all() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    manager.detach_and_shutdown_background(&sid, std::time::Duration::from_millis(50));
    assert!(manager.get_metadata(&sid).is_none());
    manager
        .shutdown_all(std::time::Duration::from_millis(50))
        .await
        .expect("shutdown_all should await detached background tasks");
}

#[test]
fn test_dormant_metadata_lifecycle() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    let meta = frontal::infra::session::SessionMetadata {
        session_id: sid.clone(),
        conversation_id: Some("conv-dormant".to_string()),
        model: Some("gemini-3.8-flash-high".to_string()),
        mode: Some("default".to_string()),
        effort: Some("high".to_string()),
        turns: 1,
        last_prompt: None,
        status: frontal::infra::session::SessionStatus::Closed,
    };
    let cfg = frontal::transport::EngineConfig {
        conversation_id: Some("conv-dormant".to_string()),
        ..Default::default()
    };
    manager.save_dormant_metadata(sid.clone(), meta, cfg);
    assert!(manager.is_dormant(&sid));
    assert!(!manager.is_replay_session(&sid));
    assert_eq!(
        manager
            .get_metadata(&sid)
            .and_then(|m| m.conversation_id.as_deref()),
        Some("conv-dormant")
    );
    assert_eq!(
        manager
            .get_engine_config(&sid)
            .and_then(|c| c.conversation_id),
        Some("conv-dormant".to_string())
    );
    manager.remove_dormant_metadata(&sid);
    assert!(!manager.is_dormant(&sid));
    assert!(manager.get_metadata(&sid).is_none());
}
