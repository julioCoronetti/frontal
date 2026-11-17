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
fn test_format_user_payload_agy_rejects_media_in_stream_json() {
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
    assert!(matches!(
        format_user_payload(&media_only),
        Err(ProtocolError::UnsupportedMedia(_))
    ));

    let combined = PromptPayload::new(
        "describe this UI",
        vec![MediaItem {
            mime_type: "image/webp".to_string(),
            uri: "/tmp/mockup.webp".to_string(),
        }],
    );
    assert!(matches!(
        format_user_payload(&combined),
        Err(ProtocolError::UnsupportedMedia(_))
    ));

    let text_only = PromptPayload::from_text("hello agy");
    let formatted_text = format_user_payload(&text_only).unwrap();
    assert!(formatted_text.contains("\"event\":\"user\""));
    assert!(formatted_text.contains("hello agy"));
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
                uri: "file://localhost/tmp/a%201.png".to_string(),
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
    assert_eq!(file_flags, vec!["/tmp/a 1.png", "/tmp/b.jpg"]);
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
async fn test_agy_engine_rejects_media_in_stream_json_mode() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let script = "#!/usr/bin/env bash\necho '{\"event\":\"init\",\"conversation_id\":\"conv-media\"}'\nread -r _line\n";
    let mock = TempFileGuard::new("mock_agy_media", "sh", script);
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
    let err = engine
        .send_prompt(PromptPayload::new(
            "check diagram",
            vec![MediaItem {
                mime_type: "image/png".to_string(),
                uri: "/tmp/diagram.png".to_string(),
            }],
        ))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Unsupported media"));
    let _ = engine.shutdown(Duration::from_millis(200)).await;
}
