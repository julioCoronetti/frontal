use frontal::protocol::TokenUsage;
use frontal::ui::modal::usage::UsageState;
use frontal::ui::modal::UsageModal;

#[test]
fn test_usage_modal_for_opencode_context_and_cache_telemetry() {
    let last = TokenUsage {
        input_tokens: 32_000,
        cache_read_tokens: 32_000,
        ..Default::default()
    };
    let acc = TokenUsage {
        input_tokens: 64_000,
        cache_read_tokens: 64_000,
        ..Default::default()
    };
    let modal = UsageModal::for_opencode("opencode-go/deepseek-v4.1-flash", Some(last), acc);
    let state = modal.state.lock().unwrap();
    let UsageState::Ready(summary) = &*state else {
        panic!("expected ready telemetry")
    };
    assert_eq!(summary.plan.tier_name, "OpenCode · DeepSeek V4.1 Flash");
    assert_eq!(summary.groups[0].name, "Context Window");
    assert!(summary.groups[0]
        .description
        .as_deref()
        .unwrap()
        .contains("window unresolved"));
    assert!(summary.groups[0].buckets.is_empty());
    assert_eq!(summary.groups[1].buckets[0].remaining_pct, 50.0);
    assert_eq!(summary.groups[2].buckets[0].name, "Accumulated");

    let resolved = UsageModal::for_opencode(
        "anthropic/claude-sonnet-5-5-high",
        Some(TokenUsage {
            input_tokens: 40_000,
            cache_read_tokens: 10_000,
            ..Default::default()
        }),
        acc,
    );
    let state = resolved.state.lock().unwrap();
    let UsageState::Ready(summary) = &*state else {
        panic!("expected ready telemetry")
    };
    assert_eq!(summary.groups[0].buckets[0].remaining_pct, 75.0);
}

#[test]
fn test_parse_go_usage_windows_and_summary_mapping() {
    let json = r#"{"usage":{"rolling":{"status":"ok","percent":42,"resetsAt":"2099-01-01T00:00:00.000Z"},"weekly":{"status":"ok","percent":18,"resetsAt":"2099-01-05T00:00:00.000Z"},"monthly":{"status":"ok","percent":100,"resetsAt":"2099-01-20T00:00:00.000Z"}}}"#;
    let usage = frontal::ui::modal::usage::opencode_api::parse_go_usage(json).unwrap();
    assert_eq!(usage.rolling.percent, 42.0);
    assert!(frontal::ui::modal::usage::opencode_api::format_reset_epoch(
        usage.rolling.resets_at_ms
    )
    .starts_with("Refreshes in "));

    let summary = frontal::ui::modal::usage::opencode_api::summary_from_go(&usage);
    assert_eq!(summary.plan.tier_name, "OpenCode Go");
    assert_eq!(summary.groups[0].name, "GO USAGE WINDOWS");
    assert_eq!(summary.groups[0].buckets[0].remaining_pct, 58.0);
    assert_eq!(summary.groups[0].buckets[1].remaining_pct, 82.0);
    assert_eq!(summary.groups[0].buckets[2].remaining_pct, 0.0);
    assert!(summary.groups[0].buckets[2].exhausted);
}

#[test]
fn test_parse_iso_ms_and_invalid_payloads() {
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_iso_ms("1970-01-02T00:00:00Z"),
        Some(86_400_000)
    );
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_iso_ms("not-a-date"),
        None
    );
    assert!(frontal::ui::modal::usage::opencode_api::parse_go_usage("{}").is_none());
}

#[test]
fn test_parse_opencode_credential_value() {
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_credential_value(
            r#"{"type":"key","key":"oc_test_value"}"#
        )
        .as_deref(),
        Some("oc_test_value")
    );
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_credential_value("{}"),
        None
    );
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_credential_value("not json"),
        None
    );
}

#[test]
fn test_model_spec_resolution_precedence_and_open_window() {
    use frontal::model_spec::{ContextWindowSource, ModelSpec};

    let overrides = vec![("opencode-go/deepseek-v4.1-flash".to_string(), 64_000u64)];
    let protocol = ModelSpec::resolve(
        &["opencode-go/deepseek-v4.1-flash"],
        Some(200_000),
        &overrides,
    );
    assert_eq!(protocol.usable(), Some(200_000));
    assert_eq!(
        protocol.context_window.unwrap().source,
        ContextWindowSource::Protocol
    );

    let config = ModelSpec::resolve(&["opencode-go/deepseek-v4.1-flash"], None, &overrides);
    assert_eq!(config.usable(), Some(64_000));
    assert_eq!(
        config.context_window.unwrap().source,
        ContextWindowSource::Config
    );

    let family = ModelSpec::resolve(&["claude-sonnet-5-5-high"], None, &[]);
    assert_eq!(family.usable(), Some(200_000));
    assert_eq!(
        family.context_window.unwrap().source,
        ContextWindowSource::Heuristic
    );

    let prioritized = ModelSpec::resolve(&["claude-sonnet-4-6", "gemini-3.1-pro-high"], None, &[]);
    assert_eq!(prioritized.usable(), Some(200_000));
    assert_eq!(
        prioritized.context_window.unwrap().source,
        ContextWindowSource::Heuristic
    );

    let provider_prefix = ModelSpec::resolve(&["gemini-proxy/llama-3-70b"], None, &[]);
    assert_eq!(provider_prefix.usable(), None);

    let unknown = ModelSpec::resolve(&["opencode-go/deepseek-v4.1-flash"], None, &[]);
    assert_eq!(unknown.usable(), None);
    assert_eq!(unknown.occupancy_pct(1_000), None);
    assert_eq!(unknown.remaining(1_000), None);
    assert_eq!(unknown.compact_margin(1_000), None);
    assert!(!unknown.is_compaction_recommended(10_000_000));
}

#[test]
fn test_model_spec_saturation_math_and_compaction_margin() {
    use frontal::model_spec::ModelSpec;

    let spec = ModelSpec::resolve(&["gemini-3.8-flash-high"], None, &[]);
    assert_eq!(spec.usable(), Some(1_000_000));
    assert_eq!(spec.occupancy_pct(500_000), Some(50.0));
    assert_eq!(spec.occupancy_pct(2_000_000), Some(100.0));
    assert_eq!(spec.remaining(1_200_000), Some(0));
    assert_eq!(spec.compaction_threshold(), Some(750_000));
    assert_eq!(spec.compact_margin(700_000), Some(50_000));
    assert_eq!(spec.compact_margin(800_000), Some(0));
    assert!(spec.is_compaction_recommended(800_000));
    assert!(!spec.is_compaction_recommended(100_000));
}

#[test]
fn test_user_config_context_window_overrides_section_and_round_trip() {
    use frontal::model_spec::ModelSpec;

    let mut config = frontal::config::UserConfig::new_isolated();
    let toml = "model = \"custom-model\" # active model\n\n[context_windows] # overrides\n\"opencode-go/deepseek-v4.1-flash\" = 64_000 # note\nclaude-sonnet-5-5-high = 1_000_000\ninvalid = nope\n";
    config.parse_content(toml);

    assert_eq!(config.model, "custom-model");
    assert_eq!(
        config.context_windows,
        vec![
            ("opencode-go/deepseek-v4.1-flash".to_string(), 64_000),
            ("claude-sonnet-5-5-high".to_string(), 1_000_000),
        ]
    );
    assert_eq!(
        ModelSpec::resolve(
            &["opencode-go/deepseek-v4.1-flash"],
            None,
            &config.context_windows
        )
        .usable(),
        Some(64_000)
    );

    let mut reloaded = frontal::config::UserConfig::new_isolated();
    reloaded.parse_content(&config.to_toml());
    assert_eq!(reloaded.context_windows, config.context_windows);
    assert_eq!(reloaded.model, "custom-model");

    config.set_context_window("opencode-go/deepseek-v4.1-flash", 128_000);
    assert_eq!(config.context_windows.len(), 2);
    assert_eq!(config.context_windows[0].1, 128_000);
}

#[test]
fn test_streaming_usage_snapshot_retention_and_consolidation() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use frontal::protocol::{
        EngineEvent, ErrorEvent, SessionEnvelope, StepUpdateData, StepUpdateEvent, StreamEvent,
    };

    let (mut app, _) = crate::common::test_app();

    let first = TokenUsage {
        input_tokens: 260,
        output_tokens: 40,
        total_tokens: 300,
        ..Default::default()
    };
    let streaming = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            usage: Some(first),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(streaming)),
    });
    assert_eq!(app.last_turn_usage.map(|u| u.total_tokens), Some(300));
    assert_eq!(app.steps_seen, 1);

    let second = TokenUsage {
        input_tokens: 50,
        output_tokens: 50,
        total_tokens: 100,
        ..Default::default()
    };
    let next_step = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(2),
            usage: Some(second),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(next_step)),
    });
    assert_eq!(app.last_turn_usage.map(|u| u.total_tokens), Some(400));
    assert_eq!(app.steps_seen, 2);

    app.editor.insert_str("next prompt");
    app.handle_terminal_event(crate::common::press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.last_turn_usage.map(|u| u.total_tokens), Some(400));

    let failure = StreamEvent::Error(ErrorEvent {
        error: Some("engine failure".to_string()),
        message: None,
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(failure)),
    });
    assert_eq!(app.last_turn_usage.map(|u| u.total_tokens), Some(400));
    assert_eq!(app.turns_completed, 0);

    let next_turn = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(0),
            usage: Some(first),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(next_turn)),
    });
    assert_eq!(app.steps_seen, 3);

    let closing = StreamEvent::Result(frontal::protocol::ResultEvent {
        conversation_id: None,
        result: Some(frontal::protocol::ResultData {
            response: Some("second turn done".to_string()),
            usage: Some(first),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(closing)),
    });
    assert_eq!(app.turns_completed, 1);
    assert_eq!(app.last_turn_usage.map(|u| u.total_tokens), Some(300));
}

#[test]
fn test_sidebar_context_telemetry_rendering_resolved_window() {
    let (mut app, _) = crate::common::test_app();
    app.cache.push_user_message("prompt for sidebar telemetry");
    app.last_turn_usage = Some(TokenUsage {
        input_tokens: 400_000,
        output_tokens: 60_000,
        cache_read_tokens: 40_000,
        total_tokens: 500_000,
        ..Default::default()
    });
    app.accumulated_usage = TokenUsage {
        input_tokens: 100_000,
        cache_read_tokens: 50_000,
        total_tokens: 150_000,
        ..Default::default()
    };
    app.steps_seen = 3;
    app.turns_completed = 2;
    app.last_turn_duration_seconds = Some(1.5);

    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("Context"));
    assert!(text.contains("500,000 tokens"));
    assert!(text.contains("50% used"));
    assert!(!text.contains("Remaining:"));
    assert!(!text.contains("Compact"));
    assert!(!text.contains("Steps:"));
    assert!(!text.contains("Cache:"));
    assert!(!text.contains("Last turn:"));
    assert!(!text.contains("Workspace"));
}

#[test]
fn test_sidebar_unknown_window_fallback() {
    let (mut app, _) = crate::common::test_app();
    app.cache.push_user_message("prompt for sidebar telemetry");
    app.user_config.model = "opencode-go/unknown-model".to_string();
    let session_id = app.active_session.clone();
    app.session_manager.set_session_metadata(
        &session_id,
        None,
        Some("opencode-go/unknown-model".to_string()),
        None,
        None,
    );
    app.last_turn_usage = Some(TokenUsage {
        input_tokens: 120_000,
        total_tokens: 120_000,
        ..Default::default()
    });

    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("120,000 tokens"));
    assert!(text.contains("window unknown"));
    assert!(!text.contains("% used"));

    app.user_config.model = "gemini-3.8-flash-high".to_string();
    let session_id = app.active_session.clone();
    app.session_manager.set_session_metadata(
        &session_id,
        None,
        Some("gemini-3.8-flash-high".to_string()),
        None,
        None,
    );
    app.last_turn_usage = Some(TokenUsage {
        input_tokens: 800_000,
        total_tokens: 800_000,
        ..Default::default()
    });

    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("800,000 tokens"));
    assert!(text.contains("80% used"));
}
