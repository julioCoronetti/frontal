use std::path::Path;

use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use frontal::infra::history::{
    load_agent_conversations_at, messages::is_custom_title_at,
    query_agent_conversation_timestamp_at, query_agent_conversation_title_at, telemetry,
    PromptHistory, SessionRecord,
};
use frontal::protocol::{
    EngineEvent, InitEvent, ResultData, ResultEvent, StepState, StepUpdateData, StepUpdateEvent,
    StreamEvent, TokenUsage,
};

use crate::common::{press_key, test_app, TempDirGuard};

fn create_agent_summaries_db(db_path: &Path) {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            last_modified_time TEXT
        );
        INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
        VALUES ('conv-ext-1', 'Alpha Title', 'alpha preview', '2026-11-18T12:00:00Z');",
    )
    .unwrap();
}

#[test]
fn test_agent_db_helpers_and_nonblocking_worker_cache() {
    let dir = TempDirGuard::new("agent_async");
    let history_db = dir.path().join("history.db");
    let agent_db = dir.path().join("conversation_summaries.db");
    create_agent_summaries_db(&agent_db);

    assert_eq!(
        query_agent_conversation_title_at(&agent_db, "conv-ext-1").as_deref(),
        Some("Alpha Title")
    );
    assert!(query_agent_conversation_timestamp_at(&agent_db, "conv-ext-1").is_some());
    assert_eq!(load_agent_conversations_at(&agent_db).len(), 1);

    let mut history =
        PromptHistory::open_with_agent_db(&history_db, Some(agent_db.clone())).unwrap();
    history.flush();

    assert_eq!(
        history.cached_agent_title("conv-ext-1").as_deref(),
        Some("Alpha Title")
    );
    assert!(history.cached_agent_timestamp("conv-ext-1").is_some());
    assert_eq!(history.cached_agent_conversations().len(), 1);

    history.record_session(SessionRecord {
        id: "tab-1".to_string(),
        conversation_id: Some("conv-ext-1".to_string()),
        model: None,
        mode: None,
        effort: None,
        title: None,
        created_at: 0,
    });
    history.rename_agent_title("conv-ext-1", "Renamed Off-Thread");
    assert_eq!(
        history.cached_agent_title("conv-ext-1").as_deref(),
        Some("Renamed Off-Thread")
    );
    history.flush();
    assert_eq!(
        query_agent_conversation_title_at(&agent_db, "conv-ext-1").as_deref(),
        Some("Renamed Off-Thread")
    );

    {
        let conn = rusqlite::Connection::open(&agent_db).unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
             VALUES ('conv-ext-2', 'Fresh Background Title', 'p2', '2026-11-18T12:05:00Z')",
            [],
        )
        .unwrap();
    }
    history.request_agent_refresh(Some("conv-ext-2".to_string()));
    history.flush();
    let updates = history.poll_agent_title_updates();
    assert_eq!(
        updates,
        vec![(
            "conv-ext-2".to_string(),
            "Fresh Background Title".to_string()
        )]
    );
    assert_eq!(
        history.cached_agent_title("conv-ext-2").as_deref(),
        Some("Fresh Background Title")
    );
}

#[test]
fn test_in_memory_history_never_queries_external_db_on_result() {
    let (mut app, sid) = test_app();
    assert!(app.history.is_in_memory());

    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: sid,
        event: EngineEvent::Stream(Box::new(StreamEvent::Result(ResultEvent {
            conversation_id: Some("conv-isolated".to_string()),
            result: Some(ResultData {
                conversation_id: Some("conv-isolated".to_string()),
                response: Some("done".to_string()),
                ..Default::default()
            }),
        }))),
    });

    assert!(!app.apply_pending_agent_updates());
    assert!(app.history.cached_agent_conversations().is_empty());
}

#[test]
fn test_rename_upserts_uncached_agent_entry_and_resists_stale_refresh() {
    let dir = TempDirGuard::new("agent_pin");
    let history_db = dir.path().join("history.db");
    let agent_db = dir.path().join("conversation_summaries.db");
    create_agent_summaries_db(&agent_db);

    {
        let conn = rusqlite::Connection::open(&agent_db).unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
             VALUES ('conv-new', 'Stale Engine Title', 'p', '2026-11-18T12:10:00Z')",
            [],
        )
        .unwrap();
    }

    let mut history =
        PromptHistory::open_with_agent_db(&history_db, Some(agent_db.clone())).unwrap();
    history.rename_agent_title("conv-new", "Custom Pinned Title");
    assert_eq!(
        history.cached_agent_title("conv-new").as_deref(),
        Some("Custom Pinned Title")
    );

    {
        let conn = rusqlite::Connection::open(&agent_db).unwrap();
        conn.execute(
            "UPDATE conversation_summaries SET title = 'Overwritten By Engine' WHERE conversation_id = 'conv-new'",
            [],
        )
        .unwrap();
    }

    history.request_agent_refresh(Some("conv-new".to_string()));
    history.request_agent_refresh(None);
    history.flush();

    assert_eq!(
        history.cached_agent_title("conv-new").as_deref(),
        Some("Custom Pinned Title")
    );
    assert!(history.poll_agent_title_updates().is_empty());
}

#[test]
fn test_custom_tab_title_survives_result_and_agent_updates() {
    let dir = TempDirGuard::new("custom_title");
    let history_db = dir.path().join("history.db");
    let agent_db = dir.path().join("conversation_summaries.db");
    create_agent_summaries_db(&agent_db);

    let (mut app, sid) = test_app();
    app.history = PromptHistory::open_with_agent_db(&history_db, Some(agent_db)).unwrap();
    app.history.flush();

    app.editor.insert_str("/rename My Pinned Tab");
    app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.tabs[0].title, "My Pinned Tab");
    assert!(app.tabs[0].custom_title);

    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: sid,
        event: EngineEvent::Stream(Box::new(StreamEvent::Result(ResultEvent {
            conversation_id: Some("conv-ext-1".to_string()),
            result: Some(ResultData {
                conversation_id: Some("conv-ext-1".to_string()),
                response: Some("done".to_string()),
                ..Default::default()
            }),
        }))),
    });
    app.history.flush();
    let _ = app.apply_pending_agent_updates();

    assert_eq!(app.tabs[0].title, "My Pinned Tab");
    assert_eq!(
        app.history.cached_agent_title("conv-ext-1").as_deref(),
        Some("My Pinned Tab")
    );
}

#[tokio::test]
async fn test_custom_title_persists_across_restart_and_resume() {
    let dir = TempDirGuard::new("custom_resume");
    let history_db = dir.path().join("history.db");
    let agent_db = dir.path().join("conversation_summaries.db");
    create_agent_summaries_db(&agent_db);

    {
        let mut history =
            PromptHistory::open_with_agent_db(&history_db, Some(agent_db.clone())).unwrap();
        history.record_session(SessionRecord {
            id: "tab-init".to_string(),
            conversation_id: Some("conv-ext-1".to_string()),
            model: None,
            mode: None,
            effort: None,
            title: Some("Alpha Title".to_string()),
            created_at: 100,
        });
        history.rename_agent_title("conv-ext-1", "User Custom Resume");
        history.flush();
    }

    {
        let conn = rusqlite::Connection::open(&agent_db).unwrap();
        conn.execute(
            "UPDATE conversation_summaries SET title = 'Engine Reset Title' WHERE conversation_id = 'conv-ext-1'",
            [],
        )
        .unwrap();
    }

    let (mut app, _) = test_app();
    app.history = PromptHistory::open_with_agent_db(&history_db, Some(agent_db)).unwrap();
    app.history.flush();

    let res = app
        .respawn_active_session(Some("conv-ext-1".to_string()), None, None, None)
        .await;
    assert!(res.is_ok());
    assert_eq!(app.tabs[0].title, "User Custom Resume");
    assert!(app.tabs[0].custom_title);

    let active_sid = app.active_session.clone();
    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: active_sid,
        event: EngineEvent::Stream(Box::new(StreamEvent::Result(ResultEvent {
            conversation_id: Some("conv-ext-1".to_string()),
            result: Some(ResultData {
                conversation_id: Some("conv-ext-1".to_string()),
                response: Some("ok".to_string()),
                ..Default::default()
            }),
        }))),
    });
    app.history.flush();
    let _ = app.apply_pending_agent_updates();
    assert_eq!(app.tabs[0].title, "User Custom Resume");
    assert!(is_custom_title_at(&history_db, "conv-ext-1"));

    let (mut app2, sid2) = test_app();
    app2.history = PromptHistory::open_with_agent_db(&history_db, None).unwrap();
    app2.editor.insert_str("/rename Pre-Init Custom");
    app2.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    app2.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: sid2,
        event: EngineEvent::Stream(Box::new(StreamEvent::Init(InitEvent {
            conversation_id: Some("conv-pre-init".to_string()),
            init: None,
        }))),
    });
    app2.history.flush();
    assert!(is_custom_title_at(&history_db, "conv-pre-init"));
}

#[test]
fn test_interrupt_persists_streaming_usage_for_resume() {
    let dir = TempDirGuard::new("interrupt_usage");
    let history_db = dir.path().join("history.db");

    let (mut app, sid) = test_app();
    app.history = PromptHistory::open_with_agent_db(&history_db, None).unwrap();
    app.session_manager.set_session_metadata(
        &sid,
        Some("conv-int-1".to_string()),
        None,
        None,
        None,
    );
    app.waiting_for_response = true;
    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: sid,
        event: EngineEvent::Stream(Box::new(StreamEvent::StepUpdate(StepUpdateEvent {
            conversation_id: Some("conv-int-1".to_string()),
            step_update: Some(StepUpdateData {
                step_index: Some(1),
                state: Some(StepState::Active),
                text_delta: Some("partial".to_string()),
                usage: Some(TokenUsage {
                    input_tokens: 100_000,
                    output_tokens: 11_091,
                    ..Default::default()
                }),
                ..Default::default()
            }),
        }))),
    });
    app.waiting_for_response = true;
    app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE));
    app.history.flush();

    let loaded = telemetry::load(&history_db, "conv-int-1").expect("telemetry persisted");
    assert_eq!(loaded.last.effective_total(), 111_091);
}
