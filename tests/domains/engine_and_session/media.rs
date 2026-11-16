use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use frontal::config::BackendKind;
use frontal::engine::{AgyEngine, OpenCodeEngine};
use frontal::protocol::{
    format_user_payload, EngineEvent, MediaItem, PromptPayload, ProtocolError, SessionId,
    StreamEvent,
};
use frontal::transport::{build_opencode_payload_args, EngineConfig};

use crate::common::{TempFileGuard, PGID_TEST_LOCK};

#[test]
fn test_format_user_payload_agy_text_and_media_lowering() {
    let empty = PromptPayload::default();
    assert_eq!(
        format_user_payload(&empty).unwrap_err(),
        ProtocolError::EmptyPrompt
    );

    let media_only = PromptPayload::new(
        "   ",
        vec![MediaItem {
            mime_type: "image/png".to_string(),
            uri: "file:///tmp/screenshot.png".to_string(),
        }],
    );
    let formatted_media_only = format_user_payload(&media_only).unwrap();
    assert!(formatted_media_only.contains("\"event\":\"user\""));
    assert!(formatted_media_only.contains("[Attached Media: /tmp/screenshot.png (image/png)]"));

    let combined = PromptPayload::new(
        "describe this UI",
        vec![MediaItem {
            mime_type: "image/webp".to_string(),
            uri: "/tmp/mockup.webp".to_string(),
        }],
    );
    let formatted_combined = format_user_payload(&combined).unwrap();
    assert!(formatted_combined
        .contains("describe this UI\\n[Attached Media: /tmp/mockup.webp (image/webp)]"));
}

#[test]
fn test_build_opencode_payload_args_emits_repeatable_file_flags_and_fallback() {
    let mut config = EngineConfig::new_isolated();
    config.backend = BackendKind::OpenCode;
    config.model = Some("opencode-go/deepseek-v4.1-flash".to_string());

    let payload = PromptPayload::new(
        "",
        vec![
            MediaItem {
                mime_type: "image/png".to_string(),
                uri: "file:///tmp/a.png".to_string(),
            },
            MediaItem {
                mime_type: "image/jpeg".to_string(),
                uri: "/tmp/b.jpg".to_string(),
            },
        ],
    );
    let args = build_opencode_payload_args(&config, Some("ses_123"), &payload);
    let dash_pos = args.iter().position(|a| a == "--").unwrap();
    assert_eq!(args[dash_pos + 1], "See attached media.");
    let file_flags: Vec<&str> = args
        .windows(2)
        .filter(|w| w[0] == "--file")
        .map(|w| w[1].as_str())
        .collect();
    assert_eq!(file_flags, vec!["/tmp/a.png", "/tmp/b.jpg"]);
    assert!(args.windows(2).any(|w| w == ["--session", "ses_123"]));
}

#[tokio::test]
async fn test_opencode_engine_send_payload_passes_file_flags_to_mock_cli() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let args_log = TempFileGuard::new("opencode_media_args", "txt", "");
    let script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > {}\necho '{{\"type\":\"step_start\",\"sessionID\":\"ses_media\"}}'\necho '{{\"type\":\"text\",\"sessionID\":\"ses_media\",\"part\":{{\"text\":\"ok\"}}}}'\n",
        args_log.path().display()
    );
    let mock = TempFileGuard::new("mock_opencode_media", "sh", &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let session_id = SessionId::new();
    let (tx, mut rx) = mpsc::channel(32);
    let mut config = EngineConfig::new_isolated();
    config.backend = BackendKind::OpenCode;
    config.opencode_bin = mock.path().to_path_buf();

    let mut engine = OpenCodeEngine::new(session_id, config, tx);
    let _ = rx.recv().await;
    engine
        .send_prompt(PromptPayload::new(
            "inspect image",
            vec![MediaItem {
                mime_type: "image/png".to_string(),
                uri: "/tmp/mock_shot.png".to_string(),
            }],
        ))
        .await
        .unwrap();

    for _ in 0..30 {
        if let Ok(Some(env)) = timeout(Duration::from_millis(200), rx.recv()).await {
            if let EngineEvent::Stream(ev) = env.event {
                if matches!(*ev, StreamEvent::Result(_)) {
                    break;
                }
            }
        }
    }

    let captured = std::fs::read_to_string(args_log.path()).unwrap();
    assert!(captured.contains("--file\n/tmp/mock_shot.png\n"));
    assert!(captured.contains("--\ninspect image"));
    let _ = engine.shutdown(Duration::from_millis(200)).await;
}

#[tokio::test]
async fn test_agy_engine_send_payload_writes_media_reference_to_stdin() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let stdin_log = TempFileGuard::new("agy_media_stdin", "ndjson", "");
    let script = format!(
        "#!/usr/bin/env bash\necho '{{\"event\":\"init\",\"conversation_id\":\"conv-media\"}}'\nIFS= read -r line\nprintf '%s\\n' \"$line\" > {}\necho '{{\"event\":\"result\",\"result\":{{\"status\":\"SUCCESS\"}}}}'\n",
        stdin_log.path().display()
    );
    let mock = TempFileGuard::new("mock_agy_media", "sh", &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let session_id = SessionId::new();
    let (tx, mut rx) = mpsc::channel(32);
    let mut config = EngineConfig::new_isolated();
    config.agy_bin = mock.path().to_path_buf();

    let mut engine = AgyEngine::spawn(session_id, config, tx).await.unwrap();
    let _ = rx.recv().await;
    engine
        .send_prompt(PromptPayload::new(
            "check diagram",
            vec![MediaItem {
                mime_type: "image/png".to_string(),
                uri: "/tmp/diagram.png".to_string(),
            }],
        ))
        .await
        .unwrap();

    for _ in 0..30 {
        if let Ok(Some(env)) = timeout(Duration::from_millis(200), rx.recv()).await {
            if let EngineEvent::Stream(ev) = env.event {
                if matches!(*ev, StreamEvent::Result(_)) {
                    break;
                }
            }
        }
    }

    let captured = std::fs::read_to_string(stdin_log.path()).unwrap();
    assert!(captured.contains("\"event\":\"user\""));
    assert!(captured.contains("check diagram"));
    assert!(captured.contains("[Attached Media: /tmp/diagram.png (image/png)]"));
    let _ = engine.shutdown(Duration::from_millis(200)).await;
}
