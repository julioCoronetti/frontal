use std::collections::HashSet;
use std::time::Duration;

use rusqlite::Connection;

use super::telemetry;

fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.flatten().collect())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    cols: &HashSet<String>,
    col_name: &str,
    col_def: &str,
) -> Result<(), rusqlite::Error> {
    if !cols.contains(col_name) {
        conn.execute(
            &format!("ALTER TABLE {} ADD COLUMN {} {}", table, col_name, col_def),
            [],
        )?;
    }
    Ok(())
}

pub(super) fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.busy_timeout(Duration::from_millis(5000))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;

    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= 1 {
        return Ok(());
    }

    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let res = (|| -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS prompt_history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 prompt TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
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
             CREATE TABLE IF NOT EXISTS deleted_sessions (
                 conversation_id TEXT PRIMARY KEY,
                 deleted_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 conversation_id TEXT NOT NULL,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 tool_name TEXT,
                 tool_args TEXT,
                 step_index INTEGER,
                 turn_meta TEXT,
                 created_at INTEGER NOT NULL
             );",
        )?;
        telemetry::ensure_schema(conn)?;

        let session_cols = table_columns(conn, "sessions")?;
        add_column_if_missing(conn, "sessions", &session_cols, "effort", "TEXT")?;
        add_column_if_missing(conn, "sessions", &session_cols, "title", "TEXT")?;
        add_column_if_missing(
            conn,
            "sessions",
            &session_cols,
            "custom_title",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(conn, "sessions", &session_cols, "backend", "TEXT")?;
        add_column_if_missing(
            conn,
            "sessions",
            &session_cols,
            "title_source",
            "INTEGER NOT NULL DEFAULT 0",
        )?;

        let message_cols = table_columns(conn, "messages")?;
        add_column_if_missing(conn, "messages", &message_cols, "turn_meta", "TEXT")?;

        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_messages_conversation
                 ON messages(conversation_id, id);
             CREATE INDEX IF NOT EXISTS idx_sessions_conversation_id
                 ON sessions(conversation_id);
             CREATE INDEX IF NOT EXISTS idx_sessions_created_at
                 ON sessions(created_at);
             UPDATE sessions SET title_source = 3 WHERE custom_title != 0 AND title_source < 3;
             UPDATE sessions SET title_source = 2
                 WHERE custom_title = 0 AND title_source = 0
                   AND title IS NOT NULL AND title != '' AND title != 'New session';
             UPDATE sessions SET backend = CASE
                 WHEN conversation_id LIKE 'ses_%' THEN 'opencode'
                 ELSE 'agy'
             END
             WHERE backend IS NULL AND conversation_id IS NOT NULL AND conversation_id != '';
             PRAGMA user_version = 1;",
        )?;
        Ok(())
    })();

    match res {
        Ok(()) => conn.execute_batch("COMMIT;"),
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(err)
        }
    }
}
