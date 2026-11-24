use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::timeout;

use frontal::config::BackendKind;
use frontal::engine::AcpEngine;
use frontal::infra::transport::run_opencode_command_with_bin;
use frontal::protocol::acp::{
    parse_acp_line, select_permission_option_id, AcpIncoming, AcpPermissionOption,
};
use frontal::protocol::{EngineEvent, ResultStatus, SessionId, StreamEvent};
use frontal::session::{SessionManager, SessionMetadata, SessionStatus};
use frontal::transport::{EngineConfig, EngineError};

use crate::common::{TempDirGuard, TempFileGuard};

fn write_acp_script(name: &str, body: &str) -> TempFileGuard {
    let guard = TempFileGuard::new(&format!("acp_lifecycle_{}", name), "sh", body);
    let mut perms = std::fs::metadata(guard.path()).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(guard.path(), perms).unwrap();
    guard
}

#[test]
fn interrupt_rejects_closed_session_without_mutating_status() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    manager.set_session_status(&sid, SessionStatus::Closed);
    let err = manager.interrupt(&sid).unwrap_err();
    assert!(matches!(err, EngineError::ProcessNotRunning));
    assert_eq!(
        manager.get_metadata(&sid).unwrap().status,
        SessionStatus::Closed
    );
}

#[test]
fn dormant_metadata_mutators_update_status_metadata_and_prompt() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    let meta = SessionMetadata {
        session_id: sid.clone(),
        conversation_id: None,
        status: SessionStatus::Ready,
        model: None,
        effort: None,
        mode: None,
        turns: 0,
        last_prompt: None,
    };
    manager.save_dormant_metadata(sid.clone(), meta, EngineConfig::default());
    manager.set_session_status(&sid, SessionStatus::Closed);
    manager.set_session_metadata(
        &sid,
        Some("conv-dormant-1".to_string()),
        Some("claude-opus".to_string()),
        Some("high".to_string()),
        Some("auto".to_string()),
    );
    manager.set_session_prompt(&sid, "hello dormant");

    let updated = manager.get_metadata(&sid).unwrap();
    assert_eq!(updated.status, SessionStatus::Closed);
    assert_eq!(updated.conversation_id.as_deref(), Some("conv-dormant-1"));
    assert_eq!(updated.model.as_deref(), Some("claude-opus"));
    assert_eq!(updated.effort.as_deref(), Some("high"));
    assert_eq!(updated.mode.as_deref(), Some("auto"));
    assert_eq!(updated.last_prompt.as_deref(), Some("hello dormant"));

    let cfg = manager.get_engine_config(&sid).unwrap();
    assert_eq!(cfg.conversation_id.as_deref(), Some("conv-dormant-1"));
    assert_eq!(cfg.model.as_deref(), Some("claude-opus"));
    assert_eq!(cfg.effort.as_deref(), Some("high"));
    assert_eq!(cfg.mode.as_deref(), Some("auto"));
}

#[tokio::test]
async fn reconfigure_preserves_live_engine_when_spawn_fails() {
    let script = write_acp_script(
        "reconf_fail",
        r#"#!/usr/bin/env bash
while IFS= read -r line; do
  id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  method=$(echo "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  if [ "$method" = "initialize" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":1}}"
  elif [ "$method" = "session/load" ] || [ "$method" = "session/new" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"sessionId\":\"s-live\"}}"
  elif [ "$method" = "session/prompt" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"stopReason\":\"end_turn\"}}"
  fi
done
"#,
    );
    let (tx, mut rx) = mpsc::channel(32);
    let sid = SessionId::new();
    let cfg = EngineConfig {
        backend: BackendKind::Agy,
        agy_bin: script.path().to_path_buf(),
        ..Default::default()
    };
    let mut engine = AcpEngine::spawn(sid, cfg, tx).await.unwrap();
    timeout(Duration::from_secs(2), async {
        while let Some(env) = rx.recv().await {
            if matches!(env.event, EngineEvent::Stream(ref s) if matches!(**s, StreamEvent::Init(_)))
            {
                break;
            }
        }
    })
    .await
    .unwrap();

    let bad_cfg = EngineConfig {
        backend: BackendKind::Agy,
        agy_bin: PathBuf::from("/nonexistent/frontal_missing_bin"),
        ..Default::default()
    };
    assert!(engine.reconfigure(bad_cfg).await.is_err());

    engine.send_prompt("still alive").await.unwrap();
    let got_result = timeout(Duration::from_secs(2), async {
        while let Some(env) = rx.recv().await {
            if matches!(env.event, EngineEvent::Stream(ref s) if matches!(**s, StreamEvent::Result(_)))
            {
                return true;
            }
        }
        false
    })
    .await
    .unwrap();
    assert!(got_result);
    let _ = engine.shutdown(Duration::from_millis(200)).await;
}

#[tokio::test]
async fn eof_mid_handshake_fails_queued_prompts_and_reaps_child() {
    let script = write_acp_script(
        "eof_hs",
        r#"#!/usr/bin/env bash
read -r line
exit 7
"#,
    );
    let (tx, mut rx) = mpsc::channel(32);
    let sid = SessionId::new();
    let cfg = EngineConfig {
        backend: BackendKind::Agy,
        agy_bin: script.path().to_path_buf(),
        ..Default::default()
    };
    let mut engine = AcpEngine::spawn(sid, cfg, tx).await.unwrap();
    let _ = engine.send_prompt("queued prompt").await;

    let mut saw_proto_err = false;
    let mut saw_exited = false;
    timeout(Duration::from_secs(3), async {
        while let Some(env) = rx.recv().await {
            match env.event {
                EngineEvent::ProtocolError { .. } => saw_proto_err = true,
                EngineEvent::ProcessExited { .. } => {
                    saw_exited = true;
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(saw_proto_err);
    assert!(saw_exited);
}

#[tokio::test]
async fn eof_mid_turn_emits_error_result_and_reaps_child() {
    let script = write_acp_script(
        "eof_turn",
        r#"#!/usr/bin/env bash
while IFS= read -r line; do
  id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  method=$(echo "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  if [ "$method" = "initialize" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":1}}"
  elif [ "$method" = "session/load" ] || [ "$method" = "session/new" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"sessionId\":\"s-turn\"}}"
  elif [ "$method" = "session/prompt" ]; then
    exit 9
  fi
done
"#,
    );
    let (tx, mut rx) = mpsc::channel(32);
    let sid = SessionId::new();
    let cfg = EngineConfig {
        backend: BackendKind::Agy,
        agy_bin: script.path().to_path_buf(),
        ..Default::default()
    };
    let mut engine = AcpEngine::spawn(sid, cfg, tx).await.unwrap();
    timeout(Duration::from_secs(2), async {
        while let Some(env) = rx.recv().await {
            if matches!(env.event, EngineEvent::Stream(ref s) if matches!(**s, StreamEvent::Init(_)))
            {
                break;
            }
        }
    })
    .await
    .unwrap();

    engine.send_prompt("crash mid turn").await.unwrap();
    let mut saw_err_result = false;
    let mut saw_exited = false;
    timeout(Duration::from_secs(3), async {
        while let Some(env) = rx.recv().await {
            match env.event {
                EngineEvent::Stream(ref s) => {
                    if let StreamEvent::Result(r) = &**s {
                        if r.result.as_ref().and_then(|d| d.status.clone())
                            == Some(ResultStatus::Error)
                        {
                            saw_err_result = true;
                        }
                    }
                }
                EngineEvent::ProcessExited { .. } => {
                    saw_exited = true;
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(saw_err_result);
    assert!(saw_exited);
}

#[tokio::test]
async fn setup_pending_unrelated_error_preserves_mode_fallback() {
    let script = write_acp_script(
        "setup_fb",
        r#"#!/usr/bin/env bash
while IFS= read -r line; do
  id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  method=$(echo "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  if [ "$method" = "initialize" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":1}}"
  elif [ "$method" = "session/load" ] || [ "$method" = "session/new" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"sessionId\":\"s-fb\",\"models\":{\"currentModelId\":\"m0\",\"availableModels\":[{\"modelId\":\"m1\"}]},\"configOptions\":[{\"id\":\"mode\",\"currentValue\":\"plan\",\"options\":[{\"value\":\"plan\"},{\"value\":\"auto\"}]}]}}"
  elif [ "$method" = "session/set_model" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32602,\"message\":\"bad model\"}}"
  elif [ "$method" = "session/set_config_option" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32602,\"message\":\"bad mode\"}}"
  fi
done
"#,
    );
    let (tx, mut rx) = mpsc::channel(32);
    let sid = SessionId::new();
    let cfg = EngineConfig {
        backend: BackendKind::Agy,
        agy_bin: script.path().to_path_buf(),
        model: Some("m1".to_string()),
        mode: Some("auto".to_string()),
        ..Default::default()
    };
    let mut engine = AcpEngine::spawn(sid, cfg, tx).await.unwrap();
    let perm_mode = timeout(Duration::from_secs(3), async {
        while let Some(env) = rx.recv().await {
            if let EngineEvent::Stream(ref s) = env.event {
                if let StreamEvent::Init(init) = &**s {
                    return init.init.as_ref().and_then(|d| d.permission_mode.clone());
                }
            }
        }
        None
    })
    .await
    .unwrap();
    assert_eq!(perm_mode.as_deref(), Some("plan"));
    let _ = engine.shutdown(Duration::from_millis(200)).await;
}

#[test]
fn acp_id_null_is_notification_and_permission_fallback_rejects_non_allow() {
    let notif_line = json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "method": "session/request_permission",
        "params": {}
    })
    .to_string();
    let parsed = parse_acp_line(&notif_line).unwrap().unwrap();
    assert!(matches!(parsed, AcpIncoming::Notification { .. }));

    let opts_reject_only = vec![
        AcpPermissionOption {
            option_id: "r1".to_string(),
            name: None,
            kind: Some("reject_once".to_string()),
        },
        AcpPermissionOption {
            option_id: "u1".to_string(),
            name: None,
            kind: None,
        },
    ];
    assert_eq!(select_permission_option_id(&opts_reject_only), None);

    let opts_with_allow = vec![
        opts_reject_only[0].clone(),
        AcpPermissionOption {
            option_id: "a1".to_string(),
            name: None,
            kind: Some("Allow_Once".to_string()),
        },
    ];
    assert_eq!(
        select_permission_option_id(&opts_with_allow).as_deref(),
        Some("a1")
    );
}

#[test]
fn opencode_timeout_kills_grandchild_holding_stdout() {
    let dir = TempDirGuard::new("opencode_grandchild_timeout");
    let bin_path = dir.path().join("fake_opencode.sh");
    std::fs::write(
        &bin_path,
        "#!/usr/bin/env bash\n(trap '' TERM; sleep 10) &\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&bin_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin_path, perms).unwrap();

    let start = std::time::Instant::now();
    let res = run_opencode_command_with_bin(&bin_path, &["models"], Duration::from_millis(120));
    assert!(res.is_err());
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn replay_concurrent_send_prompt_preserves_previous_interrupt() {
    use frontal::infra::transport::{FixtureReplayEngine, ReplayMode};
    let (tx, mut rx) = mpsc::channel(16);
    let sid = SessionId::new();
    let lines = vec![
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"one"}]}}"#.to_string(),
        r#"{"type":"result","subtype":"success","is_error":false,"result":"done1"}"#.to_string(),
        r#"{"type":"result","subtype":"success","is_error":false,"result":"done2"}"#.to_string(),
    ];
    let mut engine = FixtureReplayEngine::from_lines(sid, lines, ReplayMode::TurnGated, tx);
    engine.send_prompt("first").await.unwrap();
    engine.interrupt().unwrap();
    engine.send_prompt("second").await.unwrap();

    let mut saw_interrupted = false;
    let _ = timeout(Duration::from_secs(2), async {
        while let Some(env) = rx.recv().await {
            if matches!(env.event, EngineEvent::Stderr(ref m) if m.contains("interrupted")) {
                saw_interrupted = true;
                break;
            }
        }
    })
    .await;
    assert!(saw_interrupted);
}

#[tokio::test]
async fn replay_queued_send_prompts_both_cancelled_by_interrupt() {
    use frontal::infra::transport::{FixtureReplayEngine, ReplayMode};
    let (tx, mut rx) = mpsc::channel(16);
    let sid = SessionId::new();
    let lines = vec![
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"one"}]}}"#.to_string(),
        r#"{"type":"result","subtype":"success","is_error":false,"result":"done1"}"#.to_string(),
        r#"{"type":"result","subtype":"success","is_error":false,"result":"done2"}"#.to_string(),
    ];
    let mut engine = FixtureReplayEngine::from_lines(sid, lines, ReplayMode::TurnGated, tx);
    engine.send_prompt("first").await.unwrap();
    engine.send_prompt("second").await.unwrap();
    engine.interrupt().unwrap();

    let mut interrupted_count = 0;
    let _ = timeout(Duration::from_secs(2), async {
        while let Some(env) = rx.recv().await {
            if matches!(env.event, EngineEvent::Stderr(ref m) if m.contains("interrupted")) {
                interrupted_count += 1;
                if interrupted_count == 2 {
                    break;
                }
            }
        }
    })
    .await;
    assert_eq!(interrupted_count, 2);
}
