use tokio_util::codec::{Decoder, LinesCodec, LinesCodecError};

use frontal::cli::CliArgs;
use frontal::protocol::{
    format_user_message, parse_stream_line, ProtocolError, ResultStatus, SessionId, StepState,
    StepType, StreamEvent,
};

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

#[test]
fn test_parse_stream_line_corrupted_json() {
    let bad_json = r#"{"event":"step_update","step_update":{"#;
    let res = parse_stream_line(bad_json);
    assert!(matches!(res, Err(ProtocolError::Deserialization(_))));

    let truncated = "{\"event\":";
    let res2 = parse_stream_line(truncated);
    assert!(matches!(res2, Err(ProtocolError::Deserialization(_))));
}

#[tokio::test]
async fn test_lines_codec_resilience_16mb_limit() {
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

#[test]
fn test_cli_args_parsing_options() {
    let args = vec![
        "frontal".to_string(),
        "--headless".to_string(),
        "-p".to_string(),
        "prompt 1".to_string(),
        "--prompt".to_string(),
        "prompt 2".to_string(),
        "--model".to_string(),
        "gemini-2.5-flash".to_string(),
        "--mode".to_string(),
        "architect".to_string(),
        "--effort".to_string(),
        "high".to_string(),
        "--project".to_string(),
        "test-proj".to_string(),
    ];
    let parsed = CliArgs::parse_from(args.into_iter());
    assert!(parsed.headless);
    assert_eq!(parsed.prompts, vec!["prompt 1", "prompt 2"]);
    assert_eq!(parsed.model.as_deref(), Some("gemini-2.5-flash"));
    assert_eq!(parsed.mode.as_deref(), Some("architect"));
    assert_eq!(parsed.effort.as_deref(), Some("high"));
    assert_eq!(parsed.project.as_deref(), Some("test-proj"));
}

#[test]
fn test_user_config_defaults_and_parsing() {
    let mut config = frontal::config::UserConfig::new_isolated();
    assert_eq!(config.model, frontal::config::DEFAULT_MODEL);
    assert_eq!(config.effort, frontal::config::DEFAULT_EFFORT);
    assert_eq!(config.mode, frontal::config::DEFAULT_MODE);
    assert!(config.isolated);

    let toml = "model = \"custom-model\"\neffort = 'low'\nmode = \"ask\"\n";
    config.parse_content(toml);
    assert_eq!(config.model, "custom-model");
    assert_eq!(config.effort, "low");
    assert_eq!(config.mode, "ask");

    assert!(config.save().is_ok());
}

#[test]
fn test_cli_args_engine_config_override_precedence() {
    let mut config = frontal::transport::EngineConfig::new_isolated();
    let initial_model = config.model.clone();
    assert!(initial_model.is_some());

    let empty_args = CliArgs::parse_from(["frontal"].into_iter().map(String::from));
    if empty_args.model.is_some() {
        config.model = empty_args.model;
    }
    assert_eq!(config.model, initial_model);

    let override_args = CliArgs::parse_from(
        ["frontal", "--model", "custom-model"]
            .into_iter()
            .map(String::from),
    );
    if override_args.model.is_some() {
        config.model = override_args.model;
    }
    assert_eq!(config.model.as_deref(), Some("custom-model"));
}
