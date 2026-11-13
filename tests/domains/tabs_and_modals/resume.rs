use std::io::Cursor;

use rusqlite::Connection;

use frontal::protocol::{SessionId, StepState};
use frontal::ui::cache::{BlockKind, RenderCache};
use frontal::ui::history::{
    clean_user_prompt, hydrate_cache_from_reader, query_agent_conversation_timestamp,
    query_agent_conversation_title, PromptHistory, SessionRecord,
};
use frontal::ui::modal::{Modal, ModalAction, ModelEffortModal};

use crate::common::{test_app, TempDirGuard, PGID_TEST_LOCK};

#[test]
fn test_transcript_cleaning_and_hydration() {
    let raw = "<USER_REQUEST>\nImplement /resume hydration\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nsome metadata\n</ADDITIONAL_METADATA>";
    assert_eq!(clean_user_prompt(raw), "Implement /resume hydration");

    let simple = "Simple user command";
    assert_eq!(clean_user_prompt(simple), "Simple user command");

    let ndjson = r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","content":"<USER_REQUEST>\nFix the bug\n</USER_REQUEST>"}
{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","thinking":"Analyzing the failure mode","tool_calls":[{"name":"read_file","args":{"Path":"src/main.rs"}}]}
{"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","content":"file contents"}
{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","content":"The bug is fixed."}
"#;

    let cursor = Cursor::new(ndjson);
    let mut cache = RenderCache::new();
    let count = hydrate_cache_from_reader(cursor, &mut cache);
    assert_eq!(count, 4);

    let blocks = cache.blocks();
    assert_eq!(blocks.len(), 4);

    assert_eq!(blocks[0].kind, BlockKind::User);
    assert_eq!(blocks[0].content, "Fix the bug");
    assert!(blocks[0].consolidated);

    assert_eq!(blocks[1].kind, BlockKind::Thought);
    assert_eq!(blocks[1].content, "Analyzing the failure mode");
    assert!(blocks[1].collapsed);

    match &blocks[2].kind {
        BlockKind::Tool { name, state } => {
            assert_eq!(name, "read_file");
            assert_eq!(*state, StepState::Done);
        }
        _ => panic!("Expected Tool block"),
    }
    assert_eq!(blocks[2].content, "file contents");
    assert!(blocks[2].collapsed);

    assert_eq!(blocks[3].kind, BlockKind::Assistant);
    assert_eq!(blocks[3].content, "The bug is fixed.");
    assert!(!blocks[3].collapsed);
}

#[test]
fn test_query_agent_conversation_title_preview_fallback() {
    let _lock = PGID_TEST_LOCK.blocking_lock();
    let guard = TempDirGuard::new("resume_title_test");
    let home = guard.0.clone();
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &home);

    let agy_dir = home.join(".gemini").join("antigravity-cli");
    std::fs::create_dir_all(&agy_dir).unwrap();
    let db_path = agy_dir.join("conversation_summaries.db");

    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            last_modified_time DATETIME
        );
        INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
        VALUES ('conv-fallback', '', 'seguinte, optimize database queries', '2026-11-01 12:00:00');",
    )
    .unwrap();

    let title = query_agent_conversation_title("conv-fallback");
    assert_eq!(title.as_deref(), Some("Optimize database queries"));

    let ts = query_agent_conversation_timestamp("conv-fallback");
    assert!(ts.is_some());
    assert!(ts.unwrap() > 0);

    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }
}

#[test]
fn test_prompt_history_created_at_preservation_and_touch() {
    let guard = TempDirGuard::new("resume_history_test");
    let db_path = guard.0.join("history.db");
    let mut history = PromptHistory::open(&db_path).unwrap();

    let initial_ts = 1700000000i64;
    let rec1 = SessionRecord {
        id: "sess-1".to_string(),
        conversation_id: Some("conv-abc".to_string()),
        model: Some("gemini-3.8-flash".to_string()),
        mode: None,
        effort: None,
        title: Some("Initial Title".to_string()),
        created_at: initial_ts,
    };
    history.record_session(rec1);

    let resumable = history.resumable_sessions();
    assert_eq!(resumable.len(), 1);
    assert_eq!(resumable[0].created_at, initial_ts);

    let rec2 = SessionRecord {
        id: "sess-1".to_string(),
        conversation_id: Some("conv-abc".to_string()),
        model: Some("gemini-3.8-flash".to_string()),
        mode: None,
        effort: None,
        title: None,
        created_at: 0,
    };
    history.record_session(rec2);

    let resumable2 = history.resumable_sessions();
    assert_eq!(resumable2.len(), 1);
    assert_eq!(resumable2[0].created_at, initial_ts);
    assert_eq!(resumable2[0].title.as_deref(), Some("Initial Title"));

    history.touch_session_timestamp("conv-abc");
    let resumable3 = history.resumable_sessions();
    assert_eq!(resumable3.len(), 1);
    assert!(resumable3[0].created_at > initial_ts);
}

#[tokio::test]
async fn test_respawn_active_session_hydrates_cache_and_preserves_title() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("respawn_hydrate_test");
    let home = guard.0.clone();
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &home);

    let conv_id = "test-hydrate-conv-123";
    let brain_dir = home
        .join(".gemini")
        .join("antigravity-cli")
        .join("brain")
        .join(conv_id)
        .join(".system_generated")
        .join("logs");
    std::fs::create_dir_all(&brain_dir).unwrap();

    let transcript_path = brain_dir.join("transcript.jsonl");
    let ndjson = r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","content":"<USER_REQUEST>\nReview code architecture\n</USER_REQUEST>"}
{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","content":"All architecture checks pass."}
"#;
    std::fs::write(&transcript_path, ndjson).unwrap();

    let agy_dir = home.join(".gemini").join("antigravity-cli");
    let db_path = agy_dir.join("conversation_summaries.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(&format!(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            last_modified_time DATETIME
        );
        INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
        VALUES ('{}', 'Architecture Review', 'Review code architecture', '2026-11-01 14:00:00');",
        conv_id
    ))
    .unwrap();

    let (mut app, _session_id) = test_app();
    let mock_id = SessionId::new();
    app.session_manager.register_mock_session(mock_id.clone());
    app.active_session = mock_id.clone();

    assert!(app.cache.is_empty());

    let respawn_result = app
        .respawn_active_session(Some(conv_id.to_string()), None, None, None)
        .await;
    assert!(respawn_result.is_ok());

    assert!(!app.cache.is_empty());
    assert_eq!(app.cache.blocks().len(), 2);
    assert_eq!(app.cache.blocks()[0].kind, BlockKind::User);
    assert_eq!(app.cache.blocks()[0].content, "Review code architecture");
    assert_eq!(app.cache.blocks()[1].kind, BlockKind::Assistant);
    assert_eq!(
        app.cache.blocks()[1].content,
        "All architecture checks pass."
    );

    assert_eq!(app.tabs[app.active_tab_index].title, "Architecture Review");

    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }
}

#[tokio::test]
async fn test_resume_modal_filters_uuid_fallbacks_and_deduplicates_open_sessions() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("resume_modal_dedup_test");
    let home = guard.0.clone();
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &home);

    let agy_dir = home.join(".gemini").join("antigravity-cli");
    std::fs::create_dir_all(&agy_dir).unwrap();
    let db_path = agy_dir.join("conversation_summaries.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            last_modified_time DATETIME
        );
        INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
        VALUES
            ('orphan-uuid-999', '', '', '2026-11-01 10:00:00'),
            ('valid-conv-1', 'Valid Session', 'Valid preview', '2026-11-01 11:00:00'),
            ('open-conv-2', 'Open Session', 'Open preview', '2026-11-01 12:00:00');",
    )
    .unwrap();

    let (mut app, session_id) = test_app();
    let history_db = home.join("frontal_history.db");
    app.history = PromptHistory::open(&history_db).unwrap();
    app.session_manager.set_session_metadata(
        &session_id,
        Some("open-conv-2".to_string()),
        None,
        None,
        None,
    );
    app.tabs[0].title = "Open Session".to_string();

    app.open_resume_modal(None);

    let modal = match app.modals.top() {
        Some(frontal::ui::modal::Modal::Select(s)) => s,
        _ => panic!("Expected Select modal"),
    };

    let current_items: Vec<_> = modal
        .items
        .iter()
        .filter(|i| i.category.as_deref() == Some("Current Sessions"))
        .collect();
    assert_eq!(current_items.len(), 1);
    assert_eq!(current_items[0].title, "Open Session");
    assert!(current_items[0]
        .shortcut
        .as_deref()
        .unwrap()
        .ends_with("d ago"));

    let past_items: Vec<_> = modal
        .items
        .iter()
        .filter(|i| i.category.as_deref() == Some("Past Sessions"))
        .collect();
    assert_eq!(past_items.len(), 1);
    assert_eq!(past_items[0].title, "Valid Session");
    assert!(!past_items.iter().any(|i| i.title == "Open Session"));
    assert!(!past_items
        .iter()
        .any(|i| i.title.contains("orphan-uuid-999")));

    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }
}

#[tokio::test]
async fn test_resume_switches_backend_to_conversation_origin() {
    let (mut app, _) = test_app();
    app.user_config.backend = frontal::config::BackendKind::OpenCode;
    let new_session_id = app
        .respawn_active_session(Some("conv-agy-1".to_string()), None, None, None)
        .await
        .unwrap();
    let meta = app.session_manager.get_metadata(&new_session_id).unwrap();
    assert_eq!(meta.conversation_id.as_deref(), Some("conv-agy-1"));
    assert_eq!(app.tabs[0].backend, frontal::config::BackendKind::Agy);
    assert_eq!(app.user_config.backend, frontal::config::BackendKind::Agy);

    let (mut opencode_app, _) = test_app();
    opencode_app.user_config.backend = frontal::config::BackendKind::Agy;
    let switched = opencode_app
        .respawn_active_session(Some("ses_native".to_string()), None, None, None)
        .await
        .unwrap();
    assert_eq!(
        opencode_app
            .session_manager
            .get_metadata(&switched)
            .unwrap()
            .conversation_id
            .as_deref(),
        Some("ses_native")
    );
    assert_eq!(
        opencode_app.tabs[0].backend,
        frontal::config::BackendKind::OpenCode
    );

    let (mut model_app, initial) = test_app();
    model_app.session_manager.set_session_metadata(
        &initial,
        Some("conv-agy-2".to_string()),
        None,
        None,
        None,
    );
    model_app.user_config.backend = frontal::config::BackendKind::OpenCode;
    let respawned = model_app
        .respawn_active_session(None, Some("gemini-3.8-flash-high".to_string()), None, None)
        .await
        .unwrap();
    assert!(model_app
        .session_manager
        .get_metadata(&respawned)
        .unwrap()
        .conversation_id
        .is_none());
}

#[test]
fn test_select_model_effort_persists_modal_backend() {
    let (mut app, _) = test_app();
    let mut modal = ModelEffortModal::with_models(app.available_models.clone());
    modal.set_opencode_models(vec![(
        "opencode-go/deepseek-v4.1-flash".to_string(),
        "deepseek-v4.1-flash".to_string(),
    )]);
    modal.backend = frontal::config::BackendKind::OpenCode;
    app.modals.push(Modal::ModelEffort(modal));

    let action = app.dispatch_modal_action(ModalAction::SelectModelEffort {
        model: "opencode-go/deepseek-v4.1-flash".to_string(),
        effort: "high".to_string(),
    });
    assert_eq!(
        app.user_config.backend,
        frontal::config::BackendKind::OpenCode
    );
    assert_eq!(
        app.user_config.model,
        "opencode-go/deepseek-v4.1-flash".to_string()
    );
    assert!(matches!(
        action,
        Some(Some(frontal::ui::app::AppAction::RespawnSession { .. }))
    ));
}

#[test]
fn test_prompt_failure_unwedges_tab() {
    let (mut app, session_id) = test_app();
    app.waiting_for_response = true;
    app.cache
        .append_streaming_delta(BlockKind::Assistant, "partial output");
    app.queued_prompts.push_back("queued".to_string());
    assert!(app.cache.has_unconsolidated_blocks());

    app.handle_prompt_failure(&session_id, "opencode binary not found");

    assert!(!app.waiting_for_response);
    assert!(app.queued_prompts.is_empty());
    assert!(!app.cache.has_unconsolidated_blocks());
    assert_eq!(app.status_text, "Error received");
}

#[test]
fn test_conversation_telemetry_round_trip() {
    use frontal::infra::history::telemetry;
    use frontal::infra::history::TelemetrySnapshot;
    use frontal::protocol::TokenUsage;

    let guard = TempDirGuard::new("telemetry_round_trip");
    let db_path = guard.path().join("history.db");
    let connection = Connection::open(&db_path).unwrap();
    telemetry::ensure_schema(&connection).unwrap();

    let snapshot = TelemetrySnapshot {
        last: TokenUsage {
            total_tokens: 250_000,
            ..Default::default()
        },
        accumulated: TokenUsage {
            total_tokens: 400_000,
            ..Default::default()
        },
        steps: 4,
        turns: 2,
        duration_millis: Some(1_500),
        context_window: Some(1_000_000),
        model: Some("opencode-go/deepseek-v4.1-flash".to_string()),
    };
    telemetry::upsert(&connection, "conv-1", &snapshot).unwrap();
    drop(connection);

    assert_eq!(telemetry::load(&db_path, "conv-1"), Some(snapshot.clone()));
    assert_eq!(telemetry::load(&db_path, "missing"), None);
}

#[tokio::test]
async fn test_respawn_applies_persisted_telemetry() {
    use frontal::infra::history::StoredMessage;
    use frontal::infra::history::TelemetrySnapshot;
    use frontal::protocol::TokenUsage;
    use frontal::session::SessionManager;
    use frontal::ui::app::App;

    let _lock = PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("respawn_telemetry");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", guard.0.clone());

    let mut manager = SessionManager::new();
    let session_id = SessionId::new();
    manager.register_mock_session(session_id.clone());
    let mut app = App::new(manager, session_id);

    let conv_id = "conv-telemetry-1";
    app.history.append_messages(
        conv_id.to_string(),
        vec![StoredMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_name: None,
            tool_args: None,
            step_index: None,
        }],
    );
    app.history.record_telemetry(
        conv_id,
        TelemetrySnapshot {
            last: TokenUsage {
                total_tokens: 250_000,
                ..Default::default()
            },
            accumulated: TokenUsage {
                total_tokens: 400_000,
                ..Default::default()
            },
            steps: 4,
            turns: 2,
            duration_millis: Some(1_500),
            context_window: Some(1_000_000),
            model: Some("opencode-go/deepseek-v4.1-flash".to_string()),
        },
    );
    app.history.flush();

    let result = app
        .respawn_active_session(Some(conv_id.to_string()), None, None, None)
        .await;
    assert!(result.is_ok());
    assert_eq!(
        app.last_turn_usage.map(|usage| usage.total_tokens),
        Some(250_000)
    );
    assert_eq!(app.accumulated_usage.total_tokens, 400_000);
    assert_eq!(app.steps_seen, 4);
    assert_eq!(app.turns_completed, 2);
    assert_eq!(app.last_turn_duration_seconds, Some(1.5));
    assert_eq!(app.context_window_hint, Some(1_000_000));
    assert_eq!(
        app.served_model.as_deref(),
        Some("opencode-go/deepseek-v4.1-flash")
    );

    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("250,000 tokens"));
    assert!(text.contains("25% used"));

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    }
}
