use crossterm::event::{KeyCode, KeyModifiers};
use frontal::config::BackendKind;
use frontal::infra::history::{PromptHistory, TitleSource};
use frontal::protocol::{EngineEvent, ResultEvent, SessionEnvelope, SessionId, StreamEvent};
use frontal::session::SessionManager;
use frontal::ui::app::App;
use frontal::ui::cache::BlockKind;
use frontal::ui::modal::{Modal, ModalAction};
use rusqlite::Connection;

use crate::common::{press_key, TempDirGuard};

#[tokio::test]
async fn test_tab_provisional_title_upgrades_to_semantic_on_tick_while_preserving_rename() {
    let dir = TempDirGuard::new("tab_title_upgrade");
    let history_db = dir.path().join("history.db");
    let agent_db = dir.path().join("conversation_summaries.db");
    {
        let conn = Connection::open(&agent_db).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT,
                preview TEXT,
                last_modified_time DATETIME
            );
            INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
            VALUES ('conv-upgrade-1', '', 'Check auth flow', '2026-11-18T12:00:00Z');",
        )
        .unwrap();
    }

    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    manager.set_session_metadata(&sid, Some("conv-upgrade-1".to_string()), None, None, None);
    let mut app = App::new_isolated(manager, sid.clone());
    app.history =
        PromptHistory::open_with_external_dbs(&history_db, Some(agent_db.clone()), None).unwrap();
    app.history.flush();

    app.session_manager
        .send_prompt(&sid, "Check auth flow")
        .await
        .unwrap();
    app.handle_engine_envelope(SessionEnvelope {
        session_id: sid.clone(),
        event: EngineEvent::Stream(Box::new(StreamEvent::Result(ResultEvent {
            conversation_id: Some("conv-upgrade-1".to_string()),
            result: None,
        }))),
    });
    assert_eq!(app.tabs[0].title, "Check auth flow");
    assert_eq!(app.tabs[0].title_source, TitleSource::Provisional);

    {
        let conn = Connection::open(&agent_db).unwrap();
        conn.execute(
            "UPDATE conversation_summaries SET title = 'Auth Flow Audit' WHERE conversation_id = 'conv-upgrade-1'",
            [],
        )
        .unwrap();
    }
    app.history
        .request_agent_refresh(Some("conv-upgrade-1".to_string()));
    app.history.flush();
    assert!(app.apply_pending_agent_updates());
    assert_eq!(app.tabs[0].title, "Auth Flow Audit");
    assert_eq!(app.tabs[0].title_source, TitleSource::Semantic);

    app.active_tab_mut()
        .editor
        .insert_str("/rename Pinned Name");
    app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.tabs[0].custom_title);
    assert_eq!(app.tabs[0].title_source, TitleSource::Custom);
    {
        let conn = Connection::open(&agent_db).unwrap();
        conn.execute(
            "UPDATE conversation_summaries SET title = 'Later Semantic' WHERE conversation_id = 'conv-upgrade-1'",
            [],
        )
        .unwrap();
    }
    app.history
        .request_agent_refresh(Some("conv-upgrade-1".to_string()));
    app.history.flush();
    assert!(!app.apply_pending_agent_updates());
    assert_eq!(app.tabs[0].title, "Pinned Name");
}

#[tokio::test]
async fn test_resume_modal_lists_unified_agy_and_opencode_with_badges_and_metadata() {
    let _lock = crate::common::PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("unified_resume_modal");
    let old_home = std::env::var("HOME").ok();
    let old_xdg = std::env::var("XDG_DATA_HOME").ok();
    std::env::set_var("HOME", guard.path());
    std::env::remove_var("XDG_DATA_HOME");

    let agy_dir = guard.path().join(".gemini/antigravity-cli");
    std::fs::create_dir_all(&agy_dir).unwrap();
    let agy_db = agy_dir.join("conversation_summaries.db");
    {
        let conn = Connection::open(&agy_db).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT,
                preview TEXT,
                last_modified_time DATETIME
            );
            INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
            VALUES ('conv-agy-u1', 'Agy Unified Session', 'p', '2026-11-18T12:00:00Z');",
        )
        .unwrap();
    }

    let oc_dir = guard.path().join(".local/share/opencode");
    std::fs::create_dir_all(&oc_dir).unwrap();
    let oc_db = oc_dir.join("opencode.db");
    {
        let conn = Connection::open(&oc_db).unwrap();
        conn.execute_batch(
            r#"CREATE TABLE session_v2 (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                title TEXT,
                model TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_archived INTEGER
            );
            INSERT INTO session_v2 (id, parent_id, title, model, time_created, time_updated)
            VALUES ('ses_oc_u1', NULL, 'OpenCode Unified Session', '{"id":"deepseek-v4.1-flash","providerID":"opencode-go","variant":"max"}', 1795032813708, 1795032818345);"#,
        )
        .unwrap();
    }

    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    let mut app = App::new(manager, sid);
    app.history.flush();
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;

    app.open_resume_modal(None);
    let Modal::Select(mut select) = app.modals.pop().unwrap() else {
        panic!("Expected Select modal");
    };
    let past: Vec<_> = select
        .items
        .iter()
        .filter(|i| i.category.as_deref() == Some("Past Sessions"))
        .collect();
    assert_eq!(past.len(), 2);
    let agy_item = past.iter().find(|i| i.id == "conv-agy-u1").unwrap();
    assert_eq!(agy_item.badge.as_deref(), Some("Agy"));
    let oc_item = past.iter().find(|i| i.id == "ses_oc_u1").unwrap();
    assert_eq!(oc_item.badge.as_deref(), Some("OpenCode"));
    assert_eq!(
        oc_item.value,
        ModalAction::Respawn {
            conversation_id: Some("ses_oc_u1".to_string()),
            model: Some("opencode-go/deepseek-v4.1-flash".to_string()),
            effort: Some("max".to_string()),
            mode: None,
        }
    );

    select.query = "opencode".to_string();
    let filtered = select.filtered_indices();
    assert_eq!(filtered.len(), 1);
    assert_eq!(select.items[filtered[0]].id, "ses_oc_u1");

    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }
    if let Some(x) = old_xdg {
        std::env::set_var("XDG_DATA_HOME", x);
    }
}

#[tokio::test]
async fn test_respawn_external_opencode_hydrates_viewport_model_effort_and_usage() {
    let _lock = crate::common::PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("respawn_ext_opencode");
    let old_home = std::env::var("HOME").ok();
    let old_xdg = std::env::var("XDG_DATA_HOME").ok();
    std::env::set_var("HOME", guard.path());
    std::env::remove_var("XDG_DATA_HOME");

    let oc_dir = guard.path().join(".local/share/opencode");
    std::fs::create_dir_all(&oc_dir).unwrap();
    let oc_db = oc_dir.join("opencode.db");
    {
        let conn = Connection::open(&oc_db).unwrap();
        conn.execute_batch(
            r#"CREATE TABLE session_v2 (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                title TEXT,
                model TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_archived INTEGER
            );
            CREATE TABLE session_message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL,
                seq INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            INSERT INTO session_v2 (id, parent_id, title, model, time_created, time_updated)
            VALUES ('ses_ext_34', NULL, 'External OpenCode Chat', '{"id":"deepseek-v4.1-flash","providerID":"opencode-go","variant":"max"}', 1795032813708, 1795032818345);
            INSERT INTO session_message (id, session_id, type, seq, data)
            VALUES ('u1', 'ses_ext_34', 'user', 1, '{"text":"hello opencode"}');
            INSERT INTO session_message (id, session_id, type, seq, data)
            VALUES ('a1', 'ses_ext_34', 'assistant', 2, '{"time":{"created":1000,"completed":2200},"model":{"providerID":"opencode-go","id":"deepseek-v4.1-flash","variant":"max"},"tokens":{"input":5000,"output":120,"reasoning":30,"cache":{"read":1000}},"content":[{"type":"reasoning","text":"Thinking"},{"type":"text","text":"External answer"}]}');"#,
        )
        .unwrap();
    }

    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    let mut app = App::new_isolated(manager, sid);
    app.user_config.model = "gemini-3.8-flash-high".to_string();
    app.user_config.effort = "high".to_string();

    let new_id = app
        .respawn_active_session(Some("ses_ext_34".to_string()), None, None, None)
        .await
        .unwrap();
    assert_eq!(app.tabs[0].backend, BackendKind::OpenCode);
    assert_eq!(app.tabs[0].title, "External OpenCode Chat");
    assert_eq!(app.active_tab().cache.blocks().len(), 3);
    assert_eq!(app.active_tab().cache.blocks()[0].kind, BlockKind::User);
    assert_eq!(app.active_tab().cache.blocks()[1].kind, BlockKind::Thought);
    assert_eq!(
        app.active_tab().cache.blocks()[2].kind,
        BlockKind::Assistant
    );
    assert_eq!(
        app.active_tab().last_turn_usage.map(|u| u.input_tokens),
        Some(5000)
    );
    let meta = app.session_manager.get_metadata(&new_id).unwrap();
    assert_eq!(
        meta.model.as_deref(),
        Some("opencode-go/deepseek-v4.1-flash")
    );
    assert_eq!(meta.effort.as_deref(), Some("max"));
    assert_eq!(app.user_config.model, "gemini-3.8-flash-high");
    assert_eq!(app.user_config.effort, "high");

    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    } else {
        std::env::remove_var("HOME");
    }
    if let Some(x) = old_xdg {
        std::env::set_var("XDG_DATA_HOME", x);
    }
}

#[test]
fn test_approve_danger_enqueues_pending_engine_prompts_and_reject_clears_state() {
    let mut manager = frontal::session::SessionManager::new();
    let sid = frontal::protocol::SessionId::new();
    manager.register_mock_session(sid.clone());
    let mut app = App::new_isolated(manager, sid.clone());
    app.active_tab_mut()
        .queued_prompts
        .push_back(frontal::protocol::PromptPayload::from_text("queued-1"));
    app.active_tab_mut()
        .suspended_events
        .push(frontal::protocol::StreamEvent::Result(Default::default()));
    app.dispatch_modal_action(frontal::ui::modal::ModalAction::ApproveDanger);
    assert_eq!(app.pending_engine_prompts.len(), 1);
    assert_eq!(app.pending_engine_prompts[0].0, sid);

    app.active_tab_mut().in_flight_usage = Some(frontal::protocol::TokenUsage::default());
    app.active_tab_mut()
        .transition_turn(frontal::ui::tab::TurnEvent::Submit);
    app.active_tab_mut()
        .queued_prompts
        .push_back(frontal::protocol::PromptPayload::from_text("queued-2"));
    if let Some(Some(act)) =
        app.dispatch_modal_action(frontal::ui::modal::ModalAction::RejectDanger)
    {
        app.apply_sync_action(&act);
    }
    assert!(app.active_tab().in_flight_usage.is_none());
    assert!(app.active_tab().turn_started_at().is_none());
    assert!(app.active_tab().queued_prompts.is_empty());
    assert_eq!(
        app.session_manager.get_metadata(&sid).unwrap().status,
        frontal::session::SessionStatus::Interrupted
    );
    assert!(app.session_manager.interrupt(&sid).is_ok());
    assert_eq!(
        app.session_manager.get_metadata(&sid).unwrap().status,
        frontal::session::SessionStatus::Interrupted
    );
}
