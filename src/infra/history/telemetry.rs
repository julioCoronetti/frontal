use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::protocol::TokenUsage;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub last: TokenUsage,
    pub accumulated: TokenUsage,
    pub steps: u64,
    pub turns: u64,
    pub duration_millis: Option<u64>,
    pub context_window: Option<u64>,
    pub model: Option<String>,
}

pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversation_telemetry (
            conversation_id TEXT PRIMARY KEY,
            payload TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );",
    )
}

pub fn upsert(
    conn: &Connection,
    conversation_id: &str,
    snapshot: &TelemetrySnapshot,
) -> rusqlite::Result<()> {
    if conversation_id.trim().is_empty() {
        return Ok(());
    }
    let Ok(payload) = serde_json::to_string(snapshot) else {
        return Ok(());
    };
    conn.execute(
        "INSERT INTO conversation_telemetry (conversation_id, payload, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(conversation_id) DO UPDATE SET
             payload = excluded.payload,
             updated_at = excluded.updated_at",
        params![conversation_id, payload, now_epoch()],
    )?;
    Ok(())
}

pub fn load(path: &Path, conversation_id: &str) -> Option<TelemetrySnapshot> {
    if conversation_id.trim().is_empty() || !path.is_file() {
        return None;
    }
    let conn =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    conn.busy_timeout(std::time::Duration::from_millis(300))
        .ok()?;
    let payload: String = conn
        .query_row(
            "SELECT payload FROM conversation_telemetry WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .ok()?;
    serde_json::from_str(&payload).ok()
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
