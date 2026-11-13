use tokio_util::codec::{Decoder, LinesCodec, LinesCodecError};

use frontal::cli::CliArgs;
use frontal::protocol::{
    fnv1a_64, format_user_message, parse_engine_line, parse_stream_line, OpenCodeStreamParser,
    ProtocolError, ResultStatus, SessionId, StepState, StepType, StreamEvent,
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

    let slash = format_user_message("/plan analyze architecture").unwrap();
    assert!(slash.contains("\"content\":\"/plan analyze architecture\""));

    let unix_path = format_user_message("/var/log/syslog").unwrap();
    assert!(unix_path.contains("\"content\":\"/var/log/syslog\""));
}

#[test]
fn test_parse_stream_line_init_event() {
    let raw = r#"{"event":"init","conversation_id":"43781261-122a-4f9b-a4b6-a8070c820530","init":{"cwd":"/home/timexp/workspace/frontal","tools":["read_file","write_to_file"],"permission_mode":"always-proceed","model":"gemini-3.8-flash-high","max_context_tokens":1000000}}"#;
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
        assert_eq!(init_data.model.as_deref(), Some("gemini-3.8-flash-high"));
        assert_eq!(init_data.context_window, Some(1_000_000));
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

    let raw_thought = r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":2,"state":"ACTIVE","step_type":"thought","thought":"thinking","context_length":200000}}"#;
    let parsed_thought = parse_stream_line(raw_thought).unwrap().unwrap();
    assert_eq!(parsed_thought.thought_delta(), Some("thinking"));
    if let StreamEvent::StepUpdate(ev) = parsed_thought {
        let update = ev.step_update.unwrap();
        assert_eq!(update.context_window, Some(200_000));
        assert_eq!(update.model, None);
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }
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
    let raw = r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","response":"final answer\n","duration_seconds":1.5,"num_turns":1,"model":"gemini-3.8-flash-high","context_window":1000000,"usage":{"input_tokens":100,"output_tokens":20,"thinking_tokens":10,"cache_read_tokens":5,"total_tokens":120}}}"#;
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
        assert_eq!(res.model.as_deref(), Some("gemini-3.8-flash-high"));
        assert_eq!(res.context_window, Some(1_000_000));
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
    empty_args.apply_to_config(&mut config);
    assert_eq!(config.model, initial_model);

    let override_args = CliArgs::parse_from(
        ["frontal", "--model", "custom-model"]
            .into_iter()
            .map(String::from),
    );
    override_args.apply_to_config(&mut config);
    assert_eq!(config.model.as_deref(), Some("custom-model"));
}

#[test]
fn test_cli_args_show_version() {
    let long_flag = CliArgs::parse_from(["frontal", "--version"].into_iter().map(String::from));
    assert!(long_flag.show_version);

    let short_flag = CliArgs::parse_from(["frontal", "-V"].into_iter().map(String::from));
    assert!(short_flag.show_version);

    let none_flag = CliArgs::parse_from(["frontal"].into_iter().map(String::from));
    assert!(!none_flag.show_version);
}

#[test]
fn test_opencode_step_start_and_deltas() {
    let init = r#"{"type":"step_start","sessionID":"ses_e33f0c","part":{"type":"step-start"}}"#;
    let parsed = OpenCodeStreamParser::parse_line(init).unwrap().unwrap();
    if let StreamEvent::Init(ev) = parsed {
        assert_eq!(ev.conversation_id.as_deref(), Some("ses_e33f0c"));
        assert!(ev.init.is_none());
    } else {
        panic!("Expected StreamEvent::Init");
    }

    let text = r#"{"type":"text","sessionID":"ses_e33f0c","part":{"text":"hello"}}"#;
    let parsed = OpenCodeStreamParser::parse_line(text).unwrap().unwrap();
    assert_eq!(parsed.text_delta(), Some("hello"));
    assert_eq!(parsed.conversation_id(), Some("ses_e33f0c"));
    if let StreamEvent::StepUpdate(ev) = &parsed {
        let data = ev.step_update.as_ref().unwrap();
        assert_eq!(data.step_type, Some(StepType::AgentResponse));
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }

    let reasoning = r#"{"type":"reasoning","part":{"text":"thinking deep"}}"#;
    let parsed = OpenCodeStreamParser::parse_line(reasoning)
        .unwrap()
        .unwrap();
    assert_eq!(parsed.thought_delta(), Some("thinking deep"));

    let empty = r#"{"type":"text","part":{"text":""}}"#;
    assert!(OpenCodeStreamParser::parse_line(empty).unwrap().is_none());
}

#[test]
fn test_opencode_tool_use_lifecycle_and_empty_call_id() {
    let active = r#"{"type":"tool_use","part":{"tool":"bash","callID":"call_1","state":{"status":"running","input":{"command":"ls"}}}}"#;
    let parsed = OpenCodeStreamParser::parse_line(active).unwrap().unwrap();
    if let StreamEvent::StepUpdate(ev) = &parsed {
        let data = ev.step_update.as_ref().unwrap();
        assert_eq!(data.step_type, Some(StepType::Tool));
        assert_eq!(data.state, Some(StepState::Active));
        assert_eq!(data.step_index, Some(fnv1a_64("call_1")));
        assert_eq!(data.tool_name.as_deref(), Some("bash"));
        assert!(data.tool_info.as_ref().unwrap().parameters.is_some());
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }

    let done = r#"{"type":"tool_use","part":{"tool":"bash","callID":"call_1","state":{"status":"completed","output":"ok"}}}"#;
    let parsed_done = OpenCodeStreamParser::parse_line(done).unwrap().unwrap();
    if let StreamEvent::StepUpdate(ev) = &parsed_done {
        let data = ev.step_update.as_ref().unwrap();
        assert_eq!(data.state, Some(StepState::Done));
        assert_eq!(data.step_index, Some(fnv1a_64("call_1")));
        assert_eq!(
            data.tool_info.as_ref().unwrap().output_text().as_deref(),
            Some("ok")
        );
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }

    let failed = r#"{"type":"tool_use","part":{"tool":"bash","callID":"call_2","state":{"status":"error","error":"boom"}}}"#;
    let parsed_failed = OpenCodeStreamParser::parse_line(failed).unwrap().unwrap();
    if let StreamEvent::StepUpdate(ev) = &parsed_failed {
        let data = ev.step_update.as_ref().unwrap();
        assert_eq!(data.state, Some(StepState::Error));
        assert_eq!(
            data.tool_info.as_ref().unwrap().output_text().as_deref(),
            Some("boom")
        );
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }

    let anonymous =
        r#"{"type":"tool_use","part":{"tool":"bash","callID":"","state":{"status":"running"}}}"#;
    let parsed_anonymous = OpenCodeStreamParser::parse_line(anonymous)
        .unwrap()
        .unwrap();
    if let StreamEvent::StepUpdate(ev) = &parsed_anonymous {
        assert_eq!(ev.step_update.as_ref().unwrap().step_index, None);
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }
}

#[test]
fn test_opencode_step_finish_and_error_envelopes() {
    let finish = r#"{"type":"step_finish","sessionID":"ses_1","part":{"tokens":{"input":1200,"output":300,"reasoning":50,"cache":{"read":900,"write":0}}}}"#;
    let parsed = OpenCodeStreamParser::parse_line(finish).unwrap().unwrap();
    if let StreamEvent::StepUpdate(ev) = &parsed {
        let usage = ev.step_update.as_ref().unwrap().usage.unwrap();
        assert_eq!(usage.input_tokens, 1200);
        assert_eq!(usage.output_tokens, 300);
        assert_eq!(usage.thinking_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 900);
        assert_eq!(usage.total_tokens, 0);
    } else {
        panic!("Expected StreamEvent::StepUpdate");
    }

    let rejected = r#"{"type":"error","error":{"type":"permission.rejected","message":"Permission denied: external_directory"}}"#;
    let parsed_rejected = OpenCodeStreamParser::parse_line(rejected).unwrap().unwrap();
    if let StreamEvent::Error(ev) = parsed_rejected {
        assert_eq!(
            ev.error.as_deref(),
            Some("Permission denied: external_directory")
        );
        assert!(ev.message.is_none());
    } else {
        panic!("Expected StreamEvent::Error");
    }

    let named =
        r#"{"type":"error","error":{"name":"ProviderAuthError","data":{"message":"invalid key"}}}"#;
    let parsed_named = OpenCodeStreamParser::parse_line(named).unwrap().unwrap();
    if let StreamEvent::Error(ev) = parsed_named {
        assert_eq!(ev.error.as_deref(), Some("invalid key"));
    } else {
        panic!("Expected StreamEvent::Error");
    }
}

#[test]
fn test_parse_engine_line_dispatches_by_harness() {
    let agy_line = r#"{"event":"init","conversation_id":"c1"}"#;
    assert!(!OpenCodeStreamParser::matches(agy_line));
    assert!(OpenCodeStreamParser::matches(
        r#"{"type":"text","part":{"text":"x"}}"#
    ));

    let agy = parse_engine_line(agy_line).unwrap().unwrap();
    if let StreamEvent::Init(ev) = agy {
        assert_eq!(ev.conversation_id.as_deref(), Some("c1"));
    } else {
        panic!("Expected StreamEvent::Init");
    }

    let opencode = parse_engine_line(r#"{"type":"text","part":{"text":"hi"}}"#)
        .unwrap()
        .unwrap();
    assert_eq!(opencode.text_delta(), Some("hi"));
}

#[test]
fn test_build_opencode_args_turn_1_vs_turn_n() {
    let mut config = frontal::transport::EngineConfig::new_isolated();
    config.model = Some("opencode-go/deepseek-v4.1-flash".to_string());
    config.effort = Some("medium".to_string());

    let turn1 = frontal::transport::build_opencode_args(&config, None, "hello");
    let expected: Vec<String> = [
        "run",
        "--format",
        "json",
        "--thinking",
        "--auto",
        "--model",
        "opencode-go/deepseek-v4.1-flash#high",
        "--",
        "hello",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(turn1, expected);
    assert!(!turn1.iter().any(|arg| arg == "--session"));

    let turn_n = frontal::transport::build_opencode_args(&config, Some("ses_42"), "again");
    let session_idx = turn_n.iter().position(|arg| arg == "--session").unwrap();
    assert_eq!(turn_n[session_idx + 1], "ses_42");
    assert_eq!(turn_n.last().map(String::as_str), Some("again"));
}

#[test]
fn test_opencode_effort_variant_mapping() {
    assert_eq!(frontal::transport::opencode_variant("low"), Some("low"));
    assert_eq!(frontal::transport::opencode_variant("medium"), Some("high"));
    assert_eq!(frontal::transport::opencode_variant("high"), Some("max"));
    assert_eq!(frontal::transport::opencode_variant("xhigh"), Some("max"));
    assert_eq!(frontal::transport::opencode_variant("max"), Some("max"));
    assert_eq!(frontal::transport::opencode_variant("bogus"), None);

    assert_eq!(
        frontal::transport::model_with_variant("provider/model", Some("high")),
        "provider/model#max"
    );
    assert_eq!(
        frontal::transport::model_with_variant("provider/model#low", Some("high")),
        "provider/model#low"
    );
    assert_eq!(
        frontal::transport::model_with_variant("provider/model", None),
        "provider/model"
    );
}

#[test]
fn test_parse_opencode_models_output_and_no_static_fallback() {
    let raw = "\u{1b}[32mopencode-go/deepseek-v4.1-flash\u{1b}[0m\nopencode / gpt\n\nno-slash-line\nprovider/model/extra\n";
    let parsed = frontal::transport::parse_opencode_models_output(raw);
    assert_eq!(
        parsed,
        vec![(
            "opencode-go/deepseek-v4.1-flash".to_string(),
            "deepseek-v4.1-flash".to_string()
        )]
    );
    assert!(frontal::transport::parse_opencode_models_output("").is_empty());
    assert!(frontal::transport::parse_opencode_models_output("bogus\n").is_empty());
}

#[test]
fn test_build_opencode_args_requires_session_prefix() {
    let mut config = frontal::transport::EngineConfig::new_isolated();
    config.model = Some("opencode-go/deepseek-v4.1-flash".to_string());

    let agy_uuid = "43781261-122a-4f9b-a4b6-a8070c820530";
    let foreign = frontal::transport::build_opencode_args(&config, Some(agy_uuid), "hi");
    assert!(!foreign.iter().any(|arg| arg == "--session"));

    let native = frontal::transport::build_opencode_args(&config, Some("ses_abc"), "hi");
    assert!(native.iter().any(|arg| arg == "--session"));
}
