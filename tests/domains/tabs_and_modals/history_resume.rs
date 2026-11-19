use frontal::infra::history::StoredMessage;
use frontal::ui::cache::RenderCache;
use frontal::ui::history::{hydrate_conversation, PromptHistory, SessionRecord};

use crate::common::TempDirGuard;

#[test]
fn test_conversation_message_store_roundtrip_and_hydration() {
    let dir = TempDirGuard::new("conversation_store");
    let db = dir.path().join("history.db");
    let history = PromptHistory::open(&db).unwrap();
    let conversation = "ses_store_1".to_string();
    history.append_messages(
        conversation.clone(),
        vec![
            StoredMessage {
                role: "user".to_string(),
                content: "oi".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: None,
                turn_meta: None,
            },
            StoredMessage {
                role: "thought".to_string(),
                content: "pensando".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: Some(2),
                turn_meta: None,
            },
            StoredMessage {
                role: "assistant".to_string(),
                content: "resposta".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: None,
                turn_meta: None,
            },
        ],
    );
    history.flush();

    let stored = frontal::infra::history::conversation_messages_at(&db, &conversation);
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[0].role, "user");
    assert_eq!(stored[1].step_index, Some(2));
    assert_eq!(stored[2].content, "resposta");

    let mut cache = RenderCache::new();
    assert!(hydrate_conversation(
        &conversation,
        &mut cache,
        Some(db.as_path())
    ));
    assert_eq!(cache.blocks_count(), 3);
    assert!(!hydrate_conversation(
        "ses_missing",
        &mut cache,
        Some(db.as_path())
    ));
}

#[test]
fn test_conversation_title_fallback_from_store() {
    let dir = TempDirGuard::new("conversation_title");
    let db = dir.path().join("history.db");
    let mut history = PromptHistory::open(&db).unwrap();
    history.record_session(SessionRecord {
        id: "tab-local".to_string(),
        conversation_id: Some("ses_title".to_string()),
        model: None,
        mode: None,
        effort: None,
        title: Some("Titulo persistido".to_string()),
        created_at: 1,
        ..Default::default()
    });
    history.flush();

    assert_eq!(
        frontal::ui::history::conversation_title("ses_title", Some(db.as_path())).as_deref(),
        Some("Titulo persistido")
    );
}

#[tokio::test]
async fn test_respawn_external_agy_transcript_restores_model_effort_and_tokens_without_mutating_user_config(
) {
    use frontal::protocol::SessionId;
    use frontal::session::SessionManager;
    use frontal::ui::app::App;

    let _lock = crate::common::PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("agy_transcript_resume");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", guard.0.clone());

    let conv_id = "conv-ext-agy-41";
    let logs_dir = guard
        .path()
        .join(".gemini/antigravity-cli/brain")
        .join(conv_id)
        .join(".system_generated/logs");
    std::fs::create_dir_all(&logs_dir).unwrap();
    let transcript = [
        r#"{"type":"USER_INPUT","content":"<USER_REQUEST>\nola\n</USER_REQUEST>"}"#,
        r#"{"type":"PLANNER_RESPONSE","step_index":1,"model":"gemini-4-hydrogen-high","input_tokens":3750,"output_tokens":497,"cache_read_tokens":361795,"content":"resposta"}"#,
    ]
    .join("\n");
    std::fs::write(logs_dir.join("transcript.jsonl"), transcript).unwrap();

    let mut manager = SessionManager::new();
    let session_id = SessionId::new();
    manager.register_mock_session(session_id.clone());
    let mut app = App::new(manager, session_id);
    app.user_config.model = "opencode-go/deepseek-v4.1-flash".to_string();
    app.user_config.effort = "max".to_string();

    let new_id = app
        .respawn_active_session(Some(conv_id.to_string()), None, None, None)
        .await
        .unwrap();

    assert_eq!(app.active_model_name(), "gemini-4-hydrogen");
    assert_eq!(app.active_effort_name(), "high");
    let meta = app.session_manager.get_metadata(&new_id).unwrap();
    assert_eq!(meta.model.as_deref(), Some("gemini-4-hydrogen"));
    assert_eq!(meta.effort.as_deref(), Some("high"));
    assert_eq!(
        app.last_turn_usage.map(|u| u.effective_total()),
        Some(366_042)
    );
    assert_eq!(app.accumulated_usage.effective_total(), 366_042);
    assert_eq!(app.user_config.model, "opencode-go/deepseek-v4.1-flash");
    assert_eq!(app.user_config.effort, "max");

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
}

#[test]
fn test_resolve_session_telemetry_prefers_newer_transcript_steps_over_stale_local() {
    use frontal::infra::history::{telemetry, TelemetrySnapshot};
    use frontal::protocol::TokenUsage;

    let _lock = crate::common::PGID_TEST_LOCK.blocking_lock();
    let guard = TempDirGuard::new("stale_local_tel");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", guard.0.clone());

    let conv_id = "conv-stale-41";
    let db_path = guard.path().join("history.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    telemetry::ensure_schema(&conn).unwrap();
    telemetry::upsert(
        &conn,
        conv_id,
        &TelemetrySnapshot {
            last: TokenUsage {
                input_tokens: 1000,
                output_tokens: 100,
                total_tokens: 1100,
                ..Default::default()
            },
            accumulated: TokenUsage {
                input_tokens: 1000,
                output_tokens: 100,
                total_tokens: 1100,
                ..Default::default()
            },
            steps: 1,
            turns: 1,
            duration_millis: Some(1200),
            context_window: Some(1_048_576),
            model: Some("gemini-3.8-flash".to_string()),
        },
    )
    .unwrap();
    drop(conn);

    let logs_dir = guard
        .path()
        .join(".gemini/antigravity-cli/brain")
        .join(conv_id)
        .join(".system_generated/logs");
    std::fs::create_dir_all(&logs_dir).unwrap();
    let transcript = [
        r#"{"type":"USER_INPUT","content":"t1"}"#,
        r#"{"type":"PLANNER_RESPONSE","step_index":1,"model":"gemini-3.8-flash-medium","input_tokens":1000,"output_tokens":100,"cache_read_tokens":0}"#,
        r#"{"type":"USER_INPUT","content":"t2"}"#,
        r#"{"type":"PLANNER_RESPONSE","step_index":2,"model":"gemini-4-hydrogen-high","input_tokens":5000,"output_tokens":300,"cache_read_tokens":2000}"#,
    ]
    .join("\n");
    std::fs::write(logs_dir.join("transcript.jsonl"), transcript).unwrap();

    let resolved =
        frontal::ui::history::resolve_session_telemetry(conv_id, Some(db_path.as_path())).unwrap();
    assert_eq!(resolved.steps, 2);
    assert_eq!(resolved.turns, 2);
    assert_eq!(resolved.last.effective_total(), 7300);
    assert_eq!(resolved.model.as_deref(), Some("gemini-4-hydrogen-high"));
    assert_eq!(resolved.duration_millis, Some(1200));
    assert_eq!(resolved.context_window, Some(1_048_576));

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
}

#[tokio::test]
async fn test_respawn_opencode_session_from_agy_tab_does_not_leak_agy_model() {
    use frontal::protocol::SessionId;
    use frontal::session::SessionManager;
    use frontal::ui::app::App;

    let _lock = crate::common::PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("oc_no_leak");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", guard.0.clone());

    let mut manager = SessionManager::new();
    let session_id = SessionId::new();
    manager.register_mock_session(session_id.clone());
    let mut app = App::new_isolated(manager, session_id);
    app.user_config.backend = frontal::config::BackendKind::Agy;
    app.user_config.model = "gemini-3.8-flash-high".to_string();
    app.user_config.effort = "high".to_string();

    let new_id = app
        .respawn_active_session(Some("ses_ext_no_leak".to_string()), None, None, None)
        .await
        .unwrap();
    let meta = app.session_manager.get_metadata(&new_id).unwrap();
    assert_eq!(meta.model, None);
    assert_eq!(meta.effort, None);
    assert_eq!(app.user_config.model, "gemini-3.8-flash-high");
    assert_eq!(app.user_config.effort, "high");

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
}

#[tokio::test]
async fn test_respawn_agy_session_ignores_incompatible_opencode_user_config_model() {
    use frontal::protocol::SessionId;
    use frontal::session::SessionManager;
    use frontal::ui::app::App;

    let _lock = crate::common::PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("agy_no_oc_leak");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", guard.0.clone());

    let mut manager = SessionManager::new();
    let session_id = SessionId::new();
    manager.register_mock_session(session_id.clone());
    let mut app = App::new_isolated(manager, session_id);
    app.user_config.backend = frontal::config::BackendKind::Agy;
    app.user_config.model = "opencode-go/deepseek-v4.1-flash".to_string();
    app.user_config.effort = "max".to_string();

    let new_id = app
        .respawn_active_session(Some("conv-agy-nomodel".to_string()), None, None, None)
        .await
        .unwrap();
    let meta = app.session_manager.get_metadata(&new_id).unwrap();
    assert_eq!(meta.model, None);
    assert_eq!(meta.effort, None);

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
}
