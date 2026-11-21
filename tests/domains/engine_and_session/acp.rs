use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::timeout;

use frontal::config::BackendKind;
use frontal::engine::AcpEngine;
use frontal::protocol::acp::{
    encode_base64, format_method_not_found, format_permission_response, parse_acp_line,
    parse_prompt_result, parse_session_update, select_permission_option_id, AcpContentBlock,
    AcpIncoming, AcpPermissionOption, AcpStreamState,
};
use frontal::protocol::{
    EngineEvent, MediaItem, PromptPayload, ResultStatus, SessionId, StepState, StepType,
    StreamEvent,
};
use frontal::session::{SessionManager, SessionStatus};
use frontal::transport::EngineConfig;

use crate::common::{TempFileGuard, PGID_TEST_LOCK};

#[test]
fn test_acp_base64_rfc4648_vectors() {
    assert_eq!(encode_base64(b""), "");
    assert_eq!(encode_base64(b"f"), "Zg==");
    assert_eq!(encode_base64(b"fo"), "Zm8=");
    assert_eq!(encode_base64(b"foo"), "Zm9v");
    assert_eq!(encode_base64(b"foob"), "Zm9vYg==");
    assert_eq!(encode_base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    assert_eq!(
        encode_base64(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        "iVBORw0KGgo="
    );
}

#[test]
fn test_acp_content_block_and_wire_parsing() {
    let img = AcpContentBlock::Image {
        data: "iVBORw0KGgo=".to_string(),
        mime_type: "image/png".to_string(),
        uri: None,
    };
    let val = serde_json::to_value(&img).unwrap();
    assert_eq!(val["type"], "image");
    assert_eq!(val["mimeType"], "image/png");
    assert_eq!(val["data"], "iVBORw0KGgo=");
    assert!(val.get("uri").is_none());

    let perm_line = r#"{"jsonrpc":"2.0","id":"req-9","method":"session/request_permission","params":{"sessionId":"s-1","toolCall":{"name":"read_file"},"options":[{"optionId":"opt_once","name":"Allow Once","kind":"allow_once"},{"optionId":"opt_always","name":"Allow Always","kind":"allow_always"}]}}"#;
    match parse_acp_line(perm_line).unwrap().unwrap() {
        AcpIncoming::PermissionRequest { id, options, .. } => {
            assert_eq!(id, json!("req-9"));
            let chosen = select_permission_option_id(&options).unwrap();
            assert_eq!(chosen, "opt_always");
            let reply = format_permission_response(id, Some(&chosen)).unwrap();
            let parsed_reply: Value = serde_json::from_str(&reply).unwrap();
            assert_eq!(parsed_reply["result"]["outcome"]["outcome"], "selected");
            assert_eq!(parsed_reply["result"]["outcome"]["optionId"], "opt_always");
        }
        other => panic!("Unexpected incoming: {:?}", other),
    }

    let empty_opts: Vec<AcpPermissionOption> = vec![];
    assert_eq!(select_permission_option_id(&empty_opts), None);

    let unknown_rpc =
        r#"{"jsonrpc":"2.0","id":77,"method":"fs/read_text_file","params":{"path":"x"}}"#;
    match parse_acp_line(unknown_rpc).unwrap().unwrap() {
        AcpIncoming::UnsupportedRequest { id, method } => {
            assert_eq!(id, json!(77));
            assert_eq!(method, "fs/read_text_file");
            let reply = format_method_not_found(id, &method).unwrap();
            let parsed_reply: Value = serde_json::from_str(&reply).unwrap();
            assert_eq!(parsed_reply["error"]["code"], -32601);
        }
        other => panic!("Unexpected incoming: {:?}", other),
    }
}

#[test]
fn test_acp_session_update_and_prompt_result_translation() {
    let mut stream_state = AcpStreamState::default();

    let thought = json!({
        "sessionId": "sid-1",
        "update": { "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "analyzing png" } }
    });
    match parse_session_update(&thought, &mut stream_state).unwrap() {
        StreamEvent::StepUpdate(ev) => {
            let data = ev.step_update.unwrap();
            assert_eq!(data.step_type, Some(StepType::Thought));
            assert_eq!(data.thought.as_deref(), Some("analyzing png"));
        }
        other => panic!("Expected StepUpdate, got {:?}", other),
    }

    let usage_upd = json!({
        "sessionId": "sid-1",
        "update": { "sessionUpdate": "usage_update", "used": 4200, "size": 1048576 }
    });
    match parse_session_update(&usage_upd, &mut stream_state).unwrap() {
        StreamEvent::StepUpdate(ev) => {
            assert_eq!(ev.step_update.unwrap().context_window, Some(1_048_576));
        }
        other => panic!("Expected StepUpdate, got {:?}", other),
    }
    assert_eq!(stream_state.context_window_hint, Some(1_048_576));

    let tool_start = json!({
        "sessionId": "sid-1",
        "update": {
            "sessionUpdate": "tool_call",
            "toolCallId": "call_123",
            "name": "bash",
            "status": "in_progress",
            "rawInput": { "cmd": "ls" }
        }
    });
    match parse_session_update(&tool_start, &mut stream_state).unwrap() {
        StreamEvent::StepUpdate(ev) => {
            assert_eq!(ev.step_update.unwrap().tool_name.as_deref(), Some("bash"));
        }
        other => panic!("Expected initial tool StepUpdate, got {:?}", other),
    }

    let tool_done = json!({
        "sessionId": "sid-1",
        "update": {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call_123",
            "status": "completed",
            "rawOutput": "Cargo.toml"
        }
    });
    match parse_session_update(&tool_done, &mut stream_state).unwrap() {
        StreamEvent::StepUpdate(ev) => {
            let data = ev.step_update.unwrap();
            assert_eq!(data.step_type, Some(StepType::Tool));
            assert_eq!(data.state, Some(StepState::Done));
            assert_eq!(data.tool_name.as_deref(), Some("bash"));
            assert_eq!(
                data.tool_info.unwrap().output_text().as_deref(),
                Some("Cargo.toml")
            );
        }
        other => panic!("Expected tool StepUpdate, got {:?}", other),
    }

    let result_json = json!({
        "stopReason": "end_turn",
        "usage": {
            "inputTokens": 1200,
            "outputTokens": 45,
            "cachedReadTokens": 800,
            "thoughtTokens": 12,
            "totalTokens": 2057
        }
    });
    match parse_prompt_result(
        Some("sid-1".to_string()),
        Some("gemini-4-argon".to_string()),
        stream_state.context_window_hint,
        Some(&result_json),
        None,
    ) {
        StreamEvent::Result(res) => {
            let data = res.result.unwrap();
            assert_eq!(data.status, Some(ResultStatus::Success));
            assert_eq!(data.context_window, Some(1_048_576));
            let usage = data.usage.unwrap();
            assert_eq!(usage.input_tokens, 1200);
            assert_eq!(usage.output_tokens, 45);
            assert_eq!(usage.cache_read_tokens, 800);
            assert_eq!(usage.thinking_tokens, 12);
            assert_eq!(usage.total_tokens, 2057);
        }
        other => panic!("Expected Result, got {:?}", other),
    }

    let cancelled_json = json!({ "stopReason": "cancelled" });
    match parse_prompt_result(
        Some("sid-1".to_string()),
        None,
        None,
        Some(&cancelled_json),
        None,
    ) {
        StreamEvent::Result(res) => {
            assert_eq!(res.result.unwrap().status, Some(ResultStatus::Other));
        }
        other => panic!("Expected cancelled Result, got {:?}", other),
    }
}

#[tokio::test]
async fn test_acp_engine_handshake_image_prompt_permission_and_cancel() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let png_bytes: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x01, 0x02];
    let png_guard = TempFileGuard::new("acp_img", "png", "");
    std::fs::write(png_guard.path(), &png_bytes).unwrap();

    let wire_log = TempFileGuard::new("acp_wire_log", "jsonl", "");
    let script = format!(
        r#"#!/usr/bin/env python3
import sys, json

log_path = {:?}
with open(log_path, "w") as log:
    for line in sys.stdin:
        log.write(line)
        log.flush()
        msg = json.loads(line)
        method = msg.get("method")
        mid = msg.get("id")
        if method == "initialize":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{"protocolVersion":1}}}}), flush=True)
        elif method == "session/new":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{"sessionId":"acp-ses-42","models":{{"availableModels":[{{"modelId":"gemini-3.8-flash-high"}}],"currentModelId":"gemini-4-argon-medium"}},"configOptions":[{{"id":"mode","currentValue":"default","options":[{{"value":"default"}},{{"value":"auto"}}]}}]}}}}), flush=True)
        elif method == "session/set_model":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{}}}}), flush=True)
        elif method == "session/set_config_option":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{}}}}), flush=True)
        elif method == "session/prompt":
            print(json.dumps({{"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{{"sessionId":"acp-ses-42","toolCall":{{"name":"bash"}},"options":[{{"optionId":"allow_once","kind":"allow_once"}}]}}}}), flush=True)
            perm_reply = sys.stdin.readline()
            log.write(perm_reply)
            log.flush()
            print(json.dumps({{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-ses-42","update":{{"sessionUpdate":"usage_update","used":500,"size":1048576}}}}}}), flush=True)
            print(json.dumps({{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-ses-42","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"seen image"}}}}}}}}), flush=True)
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{"stopReason":"end_turn","usage":{{"inputTokens":100,"outputTokens":10,"cachedReadTokens":0,"thoughtTokens":2,"totalTokens":112}}}}}}), flush=True)
"#,
        wire_log.path().display().to_string()
    );
    let mock = TempFileGuard::new("mock_acp_server", "py", &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let mut config = EngineConfig::new_isolated();
    config.backend = BackendKind::Agy;
    config.agy_bin = mock.path().to_path_buf();
    config.model = Some("gemini-3.8-flash-high".to_string());
    config.effort = Some("high".to_string());
    config.mode = Some("auto".to_string());

    let mut manager = SessionManager::new();
    let sid = manager.create_subprocess_session(config).await.unwrap();

    let mut saw_init = false;
    for _ in 0..20 {
        let env = timeout(Duration::from_secs(2), manager.recv())
            .await
            .unwrap()
            .unwrap();
        if let EngineEvent::Stream(ev) = env.event {
            if matches!(*ev, StreamEvent::Init(_)) {
                saw_init = true;
                break;
            }
        }
    }
    assert!(saw_init);
    assert_eq!(
        manager
            .get_metadata(&sid)
            .unwrap()
            .conversation_id
            .as_deref(),
        Some("acp-ses-42")
    );

    let payload = PromptPayload::new(
        "inspect png",
        vec![MediaItem {
            mime_type: "image/png".to_string(),
            uri: format!("file://{}", png_guard.path().display()),
        }],
    );
    manager.send_prompt(&sid, payload).await.unwrap();

    let mut saw_text = false;
    let mut saw_result = false;
    for _ in 0..20 {
        let env = timeout(Duration::from_secs(2), manager.recv())
            .await
            .unwrap()
            .unwrap();
        if let EngineEvent::Stream(ev) = env.event {
            match *ev {
                StreamEvent::StepUpdate(u) => {
                    if u.step_update.as_ref().and_then(|s| s.text_delta.as_deref())
                        == Some("seen image")
                    {
                        saw_text = true;
                    }
                }
                StreamEvent::Result(r) => {
                    assert_eq!(
                        r.result
                            .as_ref()
                            .and_then(|d| d.usage.map(|u| u.total_tokens)),
                        Some(112)
                    );
                    saw_result = true;
                    break;
                }
                _ => {}
            }
        }
    }
    assert!(saw_text);
    assert!(saw_result);
    assert_eq!(
        manager.get_metadata(&sid).unwrap().status,
        SessionStatus::Ready
    );

    let _ = manager
        .shutdown_session(&sid, Duration::from_millis(300))
        .await;

    let logged = std::fs::read_to_string(wire_log.path()).unwrap();
    let expected_b64 = encode_base64(&png_bytes);
    assert!(logged.contains("\"method\":\"initialize\""));
    assert!(logged.contains("\"method\":\"session/new\""));
    assert!(logged.contains("\"method\":\"session/set_model\""));
    assert!(logged.contains("\"configId\":\"mode\""));
    assert!(logged.contains("\"type\":\"image\""));
    assert!(logged.contains(&expected_b64));
    assert!(logged.contains("\"optionId\":\"allow_once\""));
}

#[tokio::test]
async fn test_acp_engine_session_load_fallback_and_cancel_notification() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let wire_log = TempFileGuard::new("acp_cancel_log", "jsonl", "");
    let script = format!(
        r#"#!/usr/bin/env python3
import sys, json

log_path = {:?}
pending_prompt_id = None
with open(log_path, "w") as log:
    for line in sys.stdin:
        log.write(line)
        log.flush()
        msg = json.loads(line)
        method = msg.get("method")
        mid = msg.get("id")
        if method == "initialize":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{"protocolVersion":1}}}}), flush=True)
        elif method == "session/load":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"error":{{"code":-32000,"message":"not found"}}}}), flush=True)
        elif method == "session/new":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{"sessionId":"fallback-ses-99"}}}}), flush=True)
        elif method == "session/prompt":
            pending_prompt_id = mid
        elif method == "session/cancel":
            if pending_prompt_id is not None:
                print(json.dumps({{"jsonrpc":"2.0","id":pending_prompt_id,"result":{{"stopReason":"cancelled"}}}}), flush=True)
                pending_prompt_id = None
"#,
        wire_log.path().display().to_string()
    );
    let mock = TempFileGuard::new("mock_acp_cancel", "py", &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let sid = SessionId::new();
    let (tx, mut rx) = mpsc::channel(32);
    let mut config = EngineConfig::new_isolated();
    config.agy_bin = mock.path().to_path_buf();
    config.conversation_id = Some("missing-conv".to_string());

    let mut engine = AcpEngine::spawn(sid, config, tx).await.unwrap();
    let init_env = timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    match init_env.event {
        EngineEvent::Stream(ev) => match *ev {
            StreamEvent::Init(init) => {
                assert_eq!(init.conversation_id.as_deref(), Some("fallback-ses-99"));
            }
            other => panic!("Expected Init, got {:?}", other),
        },
        other => panic!("Expected Stream, got {:?}", other),
    }

    engine.send_prompt("long running turn").await.unwrap();
    engine.send_prompt("queued follow-up turn").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    engine.interrupt().unwrap();

    let res_env = timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        res_env.event,
        EngineEvent::Stream(ev) if matches!(*ev, StreamEvent::Result(_))
    ));
    assert!(rx.try_recv().is_err());

    let _ = engine.shutdown(Duration::from_millis(300)).await;
    let logged = std::fs::read_to_string(wire_log.path()).unwrap();
    assert!(logged.contains("\"method\":\"session/load\""));
    assert!(logged.contains("\"method\":\"session/new\""));
    assert!(logged.contains("\"method\":\"session/cancel\""));
    assert!(!logged.contains("queued follow-up turn"));
}

#[tokio::test]
async fn test_mode_backend_mapping_acp_fallback_and_cross_backend_clamp() {
    let _lock = PGID_TEST_LOCK.lock().await;
    assert!(BackendKind::Agy.supports_mode("accept-edits"));
    assert!(BackendKind::Agy.supports_mode("ACCEPT_EDITS"));
    assert!(!BackendKind::OpenCode.supports_mode("accept-edits"));
    assert!(BackendKind::OpenCode.supports_mode("plan"));

    let mut oc_cfg = EngineConfig::new_isolated();
    oc_cfg.backend = BackendKind::OpenCode;
    oc_cfg.mode = Some("plan".to_string());
    let plan_args = frontal::transport::build_opencode_args(&oc_cfg, None, "audit");
    assert!(plan_args.windows(2).any(|w| w == ["--agent", "plan"]));
    oc_cfg.mode = Some("accept-edits".to_string());
    let fallback_args = frontal::transport::build_opencode_args(&oc_cfg, None, "audit");
    assert!(!fallback_args.iter().any(|a| a == "--agent"));

    let wire_log = TempFileGuard::new("acp_mode_log", "jsonl", "");
    let script = format!(
        r#"#!/usr/bin/env python3
import sys, json

log_path = {:?}
with open(log_path, "w") as log:
    for line in sys.stdin:
        log.write(line)
        log.flush()
        msg = json.loads(line)
        method = msg.get("method")
        mid = msg.get("id")
        if method == "initialize":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{"protocolVersion":1}}}}), flush=True)
        elif method == "session/new":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{"sessionId":"acp-mode-1","configOptions":[{{"id":"mode","currentValue":"auto","options":[{{"value":"default"}},{{"value":"auto"}},{{"value":"plan"}}]}}]}}}}), flush=True)
        elif method == "session/set_config_option":
            print(json.dumps({{"jsonrpc":"2.0","id":mid,"result":{{}}}}), flush=True)
"#,
        wire_log.path().display().to_string()
    );
    let mock = TempFileGuard::new("mock_acp_mode", "py", &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let sid = SessionId::new();
    let (tx, mut rx) = mpsc::channel(16);
    let mut cfg = EngineConfig::new_isolated();
    cfg.agy_bin = mock.path().to_path_buf();
    cfg.mode = Some("accept-edits".to_string());
    let mut engine = AcpEngine::spawn(sid, cfg, tx).await.unwrap();
    let _ = timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let _ = engine.shutdown(Duration::from_millis(300)).await;
    let logged = std::fs::read_to_string(wire_log.path()).unwrap();
    assert!(logged.contains(r#""configId":"mode""#));
    assert!(logged.contains(r#""value":"default""#));

    let mut manager = SessionManager::new();
    let tab_sid = SessionId::new();
    manager.register_mock_session(tab_sid.clone());
    let mut app = frontal::ui::app::App::new_isolated(manager, tab_sid);
    app.user_config.backend = BackendKind::Agy;
    app.user_config.mode = "accept-edits".to_string();
    app.user_config.backend = BackendKind::OpenCode;
    app.respawn_active_session(None, None, None, None)
        .await
        .unwrap();
    assert_eq!(app.active_mode_name(), "auto");
}
