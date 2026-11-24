use std::time::Duration;

use rusqlite::Connection;

use frontal::infra::history::{PromptHistory, SessionRecord, TitleSource};

use crate::common::TempDirGuard;

#[test]
fn test_legacy_user_version_zero_migration_and_indexes() {
    let dir = TempDirGuard::new("history-migrations");
    let db_path = dir.path().join("history.db");

    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                 id TEXT PRIMARY KEY,
                 conversation_id TEXT,
                 backend TEXT,
                 model TEXT,
                 mode TEXT,
                 effort TEXT,
                 title TEXT,
                 custom_title INTEGER NOT NULL DEFAULT 0,
                 title_source INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 conversation_id TEXT NOT NULL,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 tool_name TEXT,
                 tool_args TEXT,
                 step_index INTEGER,
                 turn_meta TEXT,
                 created_at INTEGER NOT NULL
             );
             INSERT INTO sessions (id, conversation_id, title, custom_title, title_source, created_at)
             VALUES ('s1', 'ses_legacy1', 'Legacy Custom', 1, 0, 100);
             PRAGMA user_version = 0;",
        )
        .unwrap();
    }

    let history = PromptHistory::open_with_external_dbs(&db_path, None, None).unwrap();
    assert!(history.flush_with_timeout(Duration::from_millis(1000)));
    drop(history);

    let conn = Connection::open(&db_path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);

    let (title_source, backend): (i64, String) = conn
        .query_row(
            "SELECT title_source, backend FROM sessions WHERE id = 's1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(title_source, 3);
    assert_eq!(backend, "opencode");

    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index'")
        .unwrap();
    let indexes: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .flatten()
        .collect();
    assert!(indexes.iter().any(|n| n == "idx_sessions_conversation_id"));
    assert!(indexes.iter().any(|n| n == "idx_sessions_created_at"));
}

#[test]
fn test_title_source_can_override_matrix() {
    assert!(TitleSource::Default.can_override(TitleSource::Default));
    assert!(!TitleSource::Default.can_override(TitleSource::Provisional));
    assert!(TitleSource::Provisional.can_override(TitleSource::Default));
    assert!(TitleSource::Provisional.can_override(TitleSource::Provisional));
    assert!(!TitleSource::Provisional.can_override(TitleSource::Semantic));
    assert!(TitleSource::Semantic.can_override(TitleSource::Provisional));
    assert!(TitleSource::Semantic.can_override(TitleSource::Semantic));
    assert!(!TitleSource::Semantic.can_override(TitleSource::Custom));
    assert!(TitleSource::Custom.can_override(TitleSource::Semantic));
    assert!(TitleSource::Custom.can_override(TitleSource::Custom));
}

#[test]
fn test_history_snapshot_publishes_versioned_updates() {
    let dir = TempDirGuard::new("history-snapshot");
    let db_path = dir.path().join("history.db");
    let mut history = PromptHistory::open_with_external_dbs(&db_path, None, None).unwrap();
    let initial = history.snapshot();
    assert_eq!(initial.version, 0);

    history.record_session(SessionRecord {
        id: "tab-snap-1".to_string(),
        conversation_id: Some("conv-snap-1".to_string()),
        backend: None,
        model: None,
        mode: None,
        effort: None,
        title: Some("Snapshot Title".to_string()),
        title_source: TitleSource::Semantic,
        created_at: 1234,
    });
    assert!(history.flush_with_timeout(Duration::from_millis(1000)));
    let updated = history.snapshot();
    assert!(updated.version >= 1);
    assert!(updated
        .sessions
        .iter()
        .any(|s| s.conversation_id.as_deref() == Some("conv-snap-1")));
}
