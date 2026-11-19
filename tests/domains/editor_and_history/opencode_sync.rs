use rusqlite::Connection;

use frontal::config::BackendKind;
use frontal::infra::history::opencode::{
    load_opencode_conversations_at, load_opencode_messages_at, query_opencode_model_at,
    query_opencode_title_at,
};
use frontal::infra::history::{PromptHistory, SessionRecord, TitleSource};
use frontal::ui::cache::{BlockKind, RenderCache};
use frontal::ui::history::hydrate_conversation;

use crate::common::TempDirGuard;

fn create_opencode_db(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session_v2 (
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
        );",
    )
    .unwrap();
}

#[test]
fn test_opencode_discovery_filters_subagents_archived_and_normalizes_timestamps() {
    let dir = TempDirGuard::new("opencode_discovery");
    let db_path = dir.path().join("opencode.db");
    create_opencode_db(&db_path);

    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        r#"
        INSERT INTO session_v2 (id, parent_id, title, model, time_created, time_updated, time_archived)
        VALUES
            ('ses_top_1', NULL, 'Unified Session Sync', '{"id":"deepseek-v4.1-flash","providerID":"opencode-go","variant":"max"}', 1795032813708, 1795032818345, NULL),
            ('ses_sub_1', 'ses_top_1', 'Child Subagent', '{"id":"deepseek-v4.1-flash","providerID":"opencode-go"}', 1795032814000, 1795032819000, NULL),
            ('ses_arch_1', NULL, 'Archived Session', '{"id":"deepseek-v4.1-flash","providerID":"opencode-go"}', 1795032810000, 1795032811000, 1795032820000);
        "#,
    )
    .unwrap();

    let records = load_opencode_conversations_at(&db_path);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].conversation_id.as_deref(), Some("ses_top_1"));
    assert_eq!(records[0].backend, Some(BackendKind::OpenCode));
    assert_eq!(
        records[0].model.as_deref(),
        Some("opencode-go/deepseek-v4.1-flash")
    );
    assert_eq!(records[0].effort.as_deref(), Some("max"));
    assert_eq!(records[0].title.as_deref(), Some("Unified Session Sync"));
    assert_eq!(records[0].title_source, TitleSource::Semantic);
    assert_eq!(records[0].created_at, 1_795_032_818);

    assert_eq!(
        query_opencode_title_at(&db_path, "ses_top_1").as_deref(),
        Some("Unified Session Sync")
    );
    let (model, effort) = query_opencode_model_at(&db_path, "ses_top_1");
    assert_eq!(model.as_deref(), Some("opencode-go/deepseek-v4.1-flash"));
    assert_eq!(effort.as_deref(), Some("max"));
}

#[test]
fn test_opencode_message_hydration_and_turn_meta_parity() {
    let _lock = crate::common::PGID_TEST_LOCK.blocking_lock();
    let dir = TempDirGuard::new("opencode_hydrate");
    let old_home = std::env::var("HOME").ok();
    let old_xdg = std::env::var("XDG_DATA_HOME").ok();
    std::env::set_var("HOME", dir.path());
    std::env::remove_var("XDG_DATA_HOME");

    let oc_dir = dir.path().join(".local/share/opencode");
    std::fs::create_dir_all(&oc_dir).unwrap();
    let db_path = oc_dir.join("opencode.db");
    create_opencode_db(&db_path);

    let conn = Connection::open(&db_path).unwrap();
    let assistant_json = serde_json::json!({
        "time": {"created": 1000, "completed": 2500},
        "model": {"providerID": "opencode-go", "id": "deepseek-v4.1-flash", "variant": "max"},
        "tokens": {"input": 1200, "output": 85, "reasoning": 40, "cache": {"read": 300}},
        "content": [
            {"type": "reasoning", "text": "Checking workspace files"},
            {"type": "tool", "id": "call_1", "name": "read", "state": {"status": "completed", "input": {"path": "src/main.rs"}, "content": [{"type": "text", "text": "fn main() {}"}]}},
            {"type": "text", "text": "Here is the entry point."}
        ]
    })
    .to_string();
    conn.execute(
        "INSERT INTO session_v2 (id, parent_id, title, model, time_created, time_updated)
         VALUES ('ses_msg_1', NULL, 'Entry Point Check', '{\"id\":\"deepseek-v4.1-flash\",\"providerID\":\"opencode-go\"}', 1000, 2500)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, data) VALUES ('m1', 'ses_msg_1', 'user', 1, '{\"text\":\"show main\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, data) VALUES (?1, 'ses_msg_1', 'assistant', 2, ?2)",
        rusqlite::params!["m2", assistant_json],
    )
    .unwrap();

    let stored = load_opencode_messages_at(&db_path, "ses_msg_1");
    assert_eq!(stored.len(), 4);
    assert_eq!(stored[0].role, "user");
    assert_eq!(stored[1].role, "thought");
    assert_eq!(stored[2].role, "tool");
    assert_eq!(stored[3].role, "assistant");
    assert!(stored[3].turn_meta.is_some());

    let mut cache = RenderCache::new();
    assert!(hydrate_conversation("ses_msg_1", &mut cache, None));
    assert_eq!(cache.blocks().len(), 4);
    assert_eq!(cache.blocks()[0].kind, BlockKind::User);
    assert_eq!(cache.blocks()[1].kind, BlockKind::Thought);
    assert!(matches!(cache.blocks()[2].kind, BlockKind::Tool { ref name, .. } if name == "read"));
    assert_eq!(cache.blocks()[3].kind, BlockKind::Assistant);
    let meta = cache.blocks()[3].turn_meta.as_ref().unwrap();
    assert_eq!(meta.model, "opencode-go/deepseek-v4.1-flash");
    assert_eq!(meta.output_tokens, Some(85));

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
fn test_unified_worker_sync_persists_backend_and_upgrades_provisional_title() {
    let dir = TempDirGuard::new("worker_unified_sync");
    let history_db = dir.path().join("history.db");
    let agent_db = dir.path().join("conversation_summaries.db");
    let opencode_db = dir.path().join("opencode.db");

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
            VALUES ('conv-agy-1', '', 'first prompt fallback', '2026-11-18T12:00:00Z');",
        )
        .unwrap();
    }
    create_opencode_db(&opencode_db);
    {
        let conn = Connection::open(&opencode_db).unwrap();
        conn.execute(
            "INSERT INTO session_v2 (id, parent_id, title, model, time_created, time_updated)
             VALUES ('ses_oc_1', NULL, 'OpenCode Synced', '{\"id\":\"deepseek-v4.1-flash\",\"providerID\":\"opencode-go\",\"variant\":\"high\"}', 1795032813708, 1795032818345)",
            [],
        )
        .unwrap();
    }

    let mut history = PromptHistory::open_with_external_dbs(
        &history_db,
        Some(agent_db.clone()),
        Some(opencode_db.clone()),
    )
    .unwrap();
    history.record_session(SessionRecord {
        id: "tab-agy".to_string(),
        conversation_id: Some("conv-agy-1".to_string()),
        backend: Some(BackendKind::Agy),
        model: Some("gemini-4-hydrogen".to_string()),
        effort: Some("high".to_string()),
        title: Some("First prompt fallback".to_string()),
        title_source: TitleSource::Provisional,
        created_at: 100,
        ..Default::default()
    });
    history.request_agent_refresh(Some("conv-agy-1".to_string()));
    history.flush();
    assert!(history.poll_agent_title_updates().is_empty());

    {
        let conn = Connection::open(&agent_db).unwrap();
        conn.execute(
            "UPDATE conversation_summaries SET title = 'Semantic Agy Title' WHERE conversation_id = 'conv-agy-1'",
            [],
        )
        .unwrap();
    }
    history.request_agent_refresh(Some("conv-agy-1".to_string()));
    history.request_agent_refresh(None);
    history.flush();

    let updates = history.poll_agent_title_updates();
    assert_eq!(
        updates,
        vec![("conv-agy-1".to_string(), "Semantic Agy Title".to_string())]
    );

    let reopened = PromptHistory::open_with_external_dbs(&history_db, None, None).unwrap();
    let sessions = reopened.resumable_sessions();
    let agy = sessions
        .iter()
        .find(|s| s.conversation_id.as_deref() == Some("conv-agy-1"))
        .unwrap();
    assert_eq!(agy.backend, Some(BackendKind::Agy));
    assert_eq!(agy.title.as_deref(), Some("Semantic Agy Title"));
    assert_eq!(agy.title_source, TitleSource::Semantic);

    let oc = sessions
        .iter()
        .find(|s| s.conversation_id.as_deref() == Some("ses_oc_1"))
        .unwrap();
    assert_eq!(oc.backend, Some(BackendKind::OpenCode));
    assert_eq!(oc.model.as_deref(), Some("opencode-go/deepseek-v4.1-flash"));
    assert_eq!(oc.effort.as_deref(), Some("high"));
    assert_eq!(oc.title.as_deref(), Some("OpenCode Synced"));
}

#[test]
fn test_custom_rename_blocks_semantic_upgrade_for_opencode_and_agy() {
    let dir = TempDirGuard::new("custom_blocks_semantic");
    let history_db = dir.path().join("history.db");
    let opencode_db = dir.path().join("opencode.db");
    create_opencode_db(&opencode_db);
    {
        let conn = Connection::open(&opencode_db).unwrap();
        conn.execute(
            "INSERT INTO session_v2 (id, parent_id, title, model, time_created, time_updated)
             VALUES ('ses_pinned', NULL, 'Engine Generated Title', '{\"id\":\"deepseek-v4.1-flash\",\"providerID\":\"opencode-go\"}', 1795032813708, 1795032818345)",
            [],
        )
        .unwrap();
    }

    let mut history =
        PromptHistory::open_with_external_dbs(&history_db, None, Some(opencode_db)).unwrap();
    history.record_session(SessionRecord {
        id: "tab-oc".to_string(),
        conversation_id: Some("ses_pinned".to_string()),
        backend: Some(BackendKind::OpenCode),
        title: Some("Provisional".to_string()),
        title_source: TitleSource::Provisional,
        created_at: 100,
        ..Default::default()
    });
    history.rename_agent_title("ses_pinned", "Operator Custom Title");
    history.request_agent_refresh(Some("ses_pinned".to_string()));
    history.request_agent_refresh(None);
    history.flush();

    assert!(history.poll_agent_title_updates().is_empty());
    let reopened = PromptHistory::open_with_external_dbs(&history_db, None, None).unwrap();
    let rec = reopened
        .resumable_sessions()
        .into_iter()
        .find(|s| s.conversation_id.as_deref() == Some("ses_pinned"))
        .unwrap();
    assert_eq!(rec.title.as_deref(), Some("Operator Custom Title"));
    assert_eq!(rec.title_source, TitleSource::Custom);
}
