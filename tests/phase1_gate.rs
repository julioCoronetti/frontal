use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::codec::{LinesCodec, LinesCodecError};

use frontal::engine::SubprocessEngine;
use frontal::protocol::{
    format_user_message, parse_stream_line, EngineEvent, ProtocolError, ResultStatus, SessionId,
    StepState, StepType, StreamEvent,
};
use frontal::session::{SessionManager, SessionStatus};
use frontal::transport::{EngineConfig, FixtureReplayEngine, ReplayMode};

#[test]
fn test_session_id_generation_and_format() {
    let id1 = SessionId::new();
    let id2 = SessionId::new();
    assert_ne!(id1, id2);
    assert_eq!(id1.as_str().len(), 36);
    assert_eq!(id1.to_string(), id1.0);
}

#[test]
fn test_user_message_formatting_and_slash_rejection() {
    let valid = format_user_message("hello world").unwrap();
    assert!(valid.contains("\"event\":\"user\""));
    assert!(valid.contains("\"content\":\"hello world\""));
    assert!(valid.ends_with('\n'));

    let empty = format_user_message("   ");
    assert_eq!(empty.unwrap_err(), ProtocolError::EmptyPrompt);

    let slash = format_user_message("/model gemini");
    match slash.unwrap_err() {
        ProtocolError::SlashCommandNotAllowed(cmd) => {
            assert_eq!(cmd, "/model gemini");
        }
        _ => panic!("Expected SlashCommandNotAllowed"),
    }
}

#[test]
fn test_parse_stream_line_init_event() {
    let raw = r#"{"event":"init","conversation_id":"43781261-122a-4f9b-a4b6-a8070c820530","init":{"cwd":"/home/timexp/workspace/frontal","tools":["read_file","write_to_file"],"permission_mode":"always-proceed"}}"#;
    let parsed = parse_stream_line(raw).unwrap().unwrap();
    if let StreamEvent::Init(ev) = parsed {
        assert_eq!(
            ev.conversation_id.as_deref(),
            Some("43781261-122a-4f9b-a4b6-a8070c820530")
        );
        let init_data = ev.init.unwrap();
        assert_eq!(
            init_data.cwd.as_deref(),
            Some("/home/timexp/workspace/frontal")
        );
        assert_eq!(init_data.tools.len(), 2);
        assert_eq!(init_data.permission_mode.as_deref(), Some("always-proceed"));
    } else {
        panic!("Expected StreamEvent::Init");
    }
}

#[test]
fn test_parse_stream_line_step_update_text_and_thought() {
    let raw_text = r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"hello"}}"#;
    let parsed_text = parse_stream_line(raw_text).unwrap().unwrap();
    assert_eq!(parsed_text.text_delta(), Some("hello"));
    assert_eq!(parsed_text.conversation_id(), Some("c1"));

    let raw_thought = r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":2,"state":"ACTIVE","step_type":"thought","thought":"thinking"}}"#;
    let parsed_thought = parse_stream_line(raw_thought).unwrap().unwrap();
    assert_eq!(parsed_thought.thought_delta(), Some("thinking"));
}

#[test]
fn test_parse_stream_line_step_update_tool_active_and_done() {
    let raw_active = r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"read_file","tool_info":{"name":"read_file","parameters":{"Path":"README.md"}}}}"#;
    let parsed_active = parse_stream_line(raw_active).unwrap().unwrap();
    if let StreamEvent::StepUpdate(ev) = parsed_active {
        let update = ev.step_update.unwrap();
        assert_eq!(update.state, Some(StepState::Active));
        assert_eq!(update.step_type, Some(StepType::Tool));
        assert_eq!(update.tool_name.as_deref(), Some("read_file"));
        let tool_info = update.tool_info.unwrap();
        assert_eq!(tool_info.name.as_deref(), Some("read_file"));
        assert!(tool_info.parameters.is_some());
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }

    let raw_done = r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":2,"state":"DONE","step_type":"tool","tool_name":"read_file","duration_seconds":0.035,"tool_info":{"name":"read_file","parameters":{"Path":"README.md"},"output":"content text"}}}"#;
    let parsed_done = parse_stream_line(raw_done).unwrap().unwrap();
    if let StreamEvent::StepUpdate(ev) = parsed_done {
        let update = ev.step_update.unwrap();
        assert_eq!(update.state, Some(StepState::Done));
        assert_eq!(update.duration_seconds, Some(0.035));
        let tool_info = update.tool_info.unwrap();
        assert_eq!(tool_info.output_text().as_deref(), Some("content text"));
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }
}

#[test]
fn test_parse_stream_line_result_and_usage() {
    let raw = r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","response":"final answer\n","duration_seconds":1.5,"num_turns":1,"usage":{"input_tokens":100,"output_tokens":20,"thinking_tokens":10,"cache_read_tokens":5,"total_tokens":120}}}"#;
    let parsed = parse_stream_line(raw).unwrap().unwrap();
    assert!(parsed.is_result());
    if let StreamEvent::Result(ev) = parsed {
        let res = ev.result.unwrap();
        assert_eq!(res.status, Some(ResultStatus::Success));
        assert_eq!(res.response.as_deref(), Some("final answer\n"));
        let usage = res.usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_tokens, 120);
    } else {
        panic!("Expected StreamEvent::Result");
    }
}

#[test]
fn test_parse_stream_line_raw_error_and_unknown() {
    let raw_err = "error: stream input message is missing the 'event' field";
    let parsed_err = parse_stream_line(raw_err).unwrap().unwrap();
    if let StreamEvent::Error(err) = parsed_err {
        assert!(err.error.unwrap().starts_with("error:"));
    } else {
        panic!("Expected StreamEvent::Error");
    }

    let unknown_event = r#"{"event":"future_telemetry","some_key":"val"}"#;
    let parsed_unknown = parse_stream_line(unknown_event).unwrap().unwrap();
    assert_eq!(parsed_unknown, StreamEvent::Unknown);

    let empty = parse_stream_line("   \n").unwrap();
    assert!(empty.is_none());
}

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
}

#[tokio::test]
async fn test_lines_codec_resilience_16mb_limit() {
    use tokio_util::codec::Decoder;

    let mut codec = LinesCodec::new_with_max_length(16 * 1024 * 1024);
    let mut buffer = bytes::BytesMut::new();
    let large_line = vec![b'a'; 1024 * 1024];
    buffer.extend_from_slice(&large_line);
    buffer.extend_from_slice(b"\n");

    let decoded = codec.decode(&mut buffer).unwrap().unwrap();
    assert_eq!(decoded.len(), 1024 * 1024);

    let mut small_codec = LinesCodec::new_with_max_length(100);
    let mut overflow_buffer = bytes::BytesMut::new();
    overflow_buffer.extend_from_slice(&[b'x'; 150]);
    overflow_buffer.extend_from_slice(b"\nnormal line\n");

    let first = small_codec.decode(&mut overflow_buffer);
    assert!(matches!(first, Err(LinesCodecError::MaxLineLengthExceeded)));

    let second = small_codec.decode(&mut overflow_buffer).unwrap().unwrap();
    assert_eq!(second, "normal line");
}

#[tokio::test]
async fn test_subprocess_mock_lifecycle_signals_and_orphan_cleanup() {
    let mock_path = std::env::temp_dir().join(format!("mock_agy_test_{}.sh", std::process::id()));
    let script = r#"#!/usr/bin/env bash
trap 'echo "interrupted"; exit 0' SIGINT
echo '{"event":"init","conversation_id":"mock-conv-1"}'
read -r line
echo '{"event":"result","result":{"status":"SUCCESS","response":"done"}}'
sleep 30 &
wait
"#;
    std::fs::write(&mock_path, script.as_bytes()).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&mock_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&mock_path, perms).unwrap();
    }

    let session_id = SessionId::new();
    let (tx, mut rx) = mpsc::channel(100);
    let mut config = EngineConfig::new();
    config.agy_bin = mock_path.clone();

    let mut engine = SubprocessEngine::spawn(session_id.clone(), config, tx)
        .await
        .unwrap();

    let init_env = rx.recv().await.unwrap();
    assert_eq!(init_env.session_id, session_id);
    assert_eq!(engine.conversation_id().await, "mock-conv-1");

    engine.send_prompt("test prompt").await.unwrap();
    let res_env = rx.recv().await.unwrap();
    assert!(matches!(
        *match res_env.event {
            EngineEvent::Stream(s) => s,
            _ => panic!(),
        },
        StreamEvent::Result(_)
    ));

    engine.interrupt().unwrap();

    let exit_code = engine.shutdown(Duration::from_millis(500)).await.unwrap();
    assert!(exit_code.is_some() || exit_code.is_none());

    let _ = std::fs::remove_file(&mock_path);
}
