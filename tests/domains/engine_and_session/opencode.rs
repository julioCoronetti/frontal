use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use frontal::config::BackendKind;
use frontal::engine::OpenCodeEngine;
use frontal::protocol::{
    EngineEvent, ErrorEvent, ResultStatus, SessionEnvelope, SessionId, StreamEvent,
};
use frontal::session::{SessionManager, SessionStatus};
use frontal::transport::EngineConfig;
use frontal::ui::cache::BlockKind;
use frontal::ui::tab::{StreamOutcome, TabState};

use crate::common::{TempFileGuard, PGID_TEST_LOCK};

#[tokio::test]
async fn test_opencode_engine_multi_turn_session_persistence() {
    let marker = TempFileGuard::new("opencode_session_marker", "txt", "");
    let script = format!(
        "#!/usr/bin/env bash\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"--session\" ]; then echo seen > {}; fi\ndone\necho '{{\"type\":\"step_start\",\"sessionID\":\"ses_mock\"}}'\necho '{{\"type\":\"text\",\"sessionID\":\"ses_mock\",\"part\":{{\"text\":\"hi\"}}}}'\necho '{{\"type\":\"step_finish\",\"sessionID\":\"ses_mock\",\"part\":{{\"tokens\":{{\"input\":10,\"output\":5}}}}}}'\n",
        marker.path().display()
    );
    let mock = TempFileGuard::new("mock_opencode_test", "sh", &script);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let session_id = SessionId::new();
    let (tx, mut rx) = mpsc::channel(100);
    let mut config = EngineConfig::new_isolated();
    config.backend = BackendKind::OpenCode;
    config.opencode_bin = mock.path().to_path_buf();
    config.model = Some("opencode-go/deepseek-v4.1-flash".to_string());

    let mut engine = OpenCodeEngine::new(session_id.clone(), config, tx);
    let init_env = rx.recv().await.unwrap();
    assert_eq!(init_env.session_id, session_id);

    engine.send_prompt("first turn").await.unwrap();
    let mut first_usage = None;
    for _ in 0..50 {
        let Ok(Some(env)) = timeout(Duration::from_millis(200), rx.recv()).await else {
            continue;
        };
        if let EngineEvent::Stream(event) = env.event {
            if let StreamEvent::Result(result) = *event {
                assert_eq!(
                    result.result.as_ref().unwrap().status,
                    Some(ResultStatus::Success)
                );
                first_usage = result.result.unwrap().usage;
                break;
            }
        }
    }
    let usage = first_usage.expect("terminal Result expected");
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 5);
    assert!(std::fs::read_to_string(marker.path())
        .unwrap()
        .trim()
        .is_empty());
    assert_eq!(engine.conversation_id().await.as_deref(), Some("ses_mock"));

    engine.send_prompt("second turn").await.unwrap();
    let mut second_result = false;
    for _ in 0..50 {
        let Ok(Some(env)) = timeout(Duration::from_millis(200), rx.recv()).await else {
            continue;
        };
        if let EngineEvent::Stream(event) = env.event {
            if matches!(*event, StreamEvent::Result(_)) {
                second_result = true;
                break;
            }
        }
    }
    assert!(second_result);
    assert!(std::fs::read_to_string(marker.path())
        .unwrap()
        .contains("seen"));

    let _ = engine.shutdown(Duration::from_millis(200)).await;
}

#[tokio::test]
async fn test_error_event_unwedges_session_and_tab() {
    let mut tab = TabState::new(SessionId::new(), String::from("tab"));
    tab.waiting_for_response = true;
    tab.cache
        .append_streaming_delta(BlockKind::Assistant, "partial output");
    assert!(tab.cache.has_unconsolidated_blocks());

    let error = StreamEvent::Error(ErrorEvent {
        error: Some("provider failed".to_string()),
        message: None,
    });
    let outcome = tab.apply_stream_event(&error, "auto", "model", true);
    assert_eq!(
        outcome,
        Some(StreamOutcome::TurnCompleted { is_result: false })
    );
    assert!(!tab.waiting_for_response);
    assert!(!tab.cache.has_unconsolidated_blocks());

    let mut manager = SessionManager::new();
    let session_id = SessionId::new();
    manager.register_mock_session(session_id.clone());
    manager.set_session_status(&session_id, SessionStatus::RunningTurn);
    let tx = manager.event_sender();
    tx.send(SessionEnvelope {
        session_id: session_id.clone(),
        event: EngineEvent::Stream(Box::new(error)),
    })
    .await
    .unwrap();
    let envelope = manager.recv().await.unwrap();
    assert_eq!(envelope.session_id, session_id);
    assert_eq!(
        manager.get_metadata(&session_id).unwrap().status,
        SessionStatus::Ready
    );
}

#[tokio::test]
async fn test_opencode_interrupt_emits_terminal_result_and_releases_pgid() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let script = "#!/usr/bin/env bash\ntrap 'exit 130' SIGINT\necho '{\"type\":\"step_start\",\"sessionID\":\"ses_int\"}'\nsleep 30\n";
    let mock = TempFileGuard::new("mock_opencode_interrupt", "sh", script);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let mut manager = SessionManager::new();
    let mut config = EngineConfig::new_isolated();
    config.backend = BackendKind::OpenCode;
    config.opencode_bin = mock.path().to_path_buf();
    config.model = Some("opencode-go/deepseek-v4.1-flash".to_string());
    let session_id = manager.create_subprocess_session(config).await.unwrap();
    let _ = manager.recv().await;

    manager
        .send_prompt(&session_id, "interrupt me")
        .await
        .unwrap();
    assert_eq!(
        manager.get_metadata(&session_id).unwrap().status,
        SessionStatus::RunningTurn
    );
    manager.interrupt(&session_id).unwrap();

    let mut terminal = false;
    for _ in 0..60 {
        let Ok(Some(envelope)) = timeout(Duration::from_millis(100), manager.recv()).await else {
            continue;
        };
        if let EngineEvent::Stream(event) = envelope.event {
            if event.is_result() || matches!(&*event, StreamEvent::Error(_)) {
                terminal = true;
                break;
            }
        }
    }
    assert!(terminal);
    assert_eq!(
        manager.get_metadata(&session_id).unwrap().status,
        SessionStatus::Ready
    );
}

#[tokio::test]
async fn test_opencode_engine_emits_single_terminal_event_on_stream_error() {
    let script = "#!/usr/bin/env bash\necho '{\"type\":\"step_start\",\"sessionID\":\"ses_err\"}'\necho '{\"type\":\"error\",\"error\":{\"type\":\"permission.rejected\",\"message\":\"denied\"}}'\n";
    let mock = TempFileGuard::new("mock_opencode_error", "sh", script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let (tx, mut rx) = mpsc::channel(100);
    let mut config = EngineConfig::new_isolated();
    config.backend = BackendKind::OpenCode;
    config.opencode_bin = mock.path().to_path_buf();
    let mut engine = OpenCodeEngine::new(SessionId::new(), config, tx);
    let _ = rx.recv().await;
    engine.send_prompt("trigger").await.unwrap();

    let mut terminals = Vec::new();
    for _ in 0..20 {
        match timeout(Duration::from_millis(150), rx.recv()).await {
            Ok(Some(envelope)) => {
                if let EngineEvent::Stream(event) = envelope.event {
                    if event.is_result() || matches!(&*event, StreamEvent::Error(_)) {
                        terminals.push(*event);
                    }
                }
            }
            _ => break,
        }
    }
    assert_eq!(terminals.len(), 1);
    assert!(matches!(terminals[0], StreamEvent::Error(_)));
}

#[tokio::test]
async fn test_opencode_engine_reports_non_zero_exit_as_error() {
    let mock = TempFileGuard::new(
        "mock_opencode_exit",
        "sh",
        "#!/usr/bin/env bash\necho 'boom' >&2\nexit 3\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let (tx, mut rx) = mpsc::channel(100);
    let mut config = EngineConfig::new_isolated();
    config.backend = BackendKind::OpenCode;
    config.opencode_bin = mock.path().to_path_buf();
    let mut engine = OpenCodeEngine::new(SessionId::new(), config, tx);
    let _ = rx.recv().await;
    engine.send_prompt("fails").await.unwrap();

    let mut message = None;
    for _ in 0..20 {
        match timeout(Duration::from_millis(150), rx.recv()).await {
            Ok(Some(envelope)) => {
                if let EngineEvent::Stream(event) = envelope.event {
                    if let StreamEvent::Error(err) = *event {
                        message = err.error;
                        break;
                    }
                }
            }
            _ => break,
        }
    }
    let message = message.expect("expected a stream error");
    assert!(message.contains("code 3"));
    assert!(message.contains("boom"));
}
