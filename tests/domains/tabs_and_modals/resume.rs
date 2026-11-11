use std::io::Cursor;

use rusqlite::Connection;

use frontal::protocol::{SessionId, StepState};
use frontal::ui::cache::{BlockKind, RenderCache};
use frontal::ui::history::{
    clean_user_prompt, hydrate_cache_from_reader, query_agent_conversation_timestamp,
    query_agent_conversation_title, PromptHistory, SessionRecord,
};

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
async fn test_switching_backend_clears_conversation_id() {
    let (mut app, _) = test_app();
    app.user_config.backend = frontal::config::BackendKind::OpenCode;
    let new_session_id = app
        .respawn_active_session(Some("conv-agy-1".to_string()), None, None, None)
        .await
        .unwrap();
    let meta = app.session_manager.get_metadata(&new_session_id).unwrap();
    assert!(meta.conversation_id.is_none());
    assert_eq!(app.tabs[0].backend, frontal::config::BackendKind::OpenCode);

    let (mut same_backend_app, _) = test_app();
    same_backend_app.user_config.backend = frontal::config::BackendKind::Agy;
    let preserved = same_backend_app
        .respawn_active_session(Some("conv-agy-2".to_string()), None, None, None)
        .await
        .unwrap();
    assert_eq!(
        same_backend_app
            .session_manager
            .get_metadata(&preserved)
            .unwrap()
            .conversation_id
            .as_deref(),
        Some("conv-agy-2")
    );
}
