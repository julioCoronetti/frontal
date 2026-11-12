use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredMessage {
    pub role: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub step_index: Option<u64>,
}

pub fn default_history_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".frontal").join("history.db"))
}

pub fn conversation_messages_at(path: &Path, conv_id: &str) -> Vec<StoredMessage> {
    read_connection(path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT role, content, tool_name, tool_args, step_index
             FROM messages WHERE conversation_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![conv_id], |row| {
            Ok(StoredMessage {
                role: row.get(0)?,
                content: row.get(1)?,
                tool_name: row.get(2)?,
                tool_args: row.get(3)?,
                step_index: row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
}

pub fn conversation_title_at(path: &Path, conv_id: &str) -> Option<String> {
    read_connection(path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT title FROM sessions
             WHERE conversation_id = ?1 AND title IS NOT NULL AND title != ''
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![conv_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let title: String = row.get(0).unwrap_or_default();
        let trimmed = title.trim();
        Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
    })
    .flatten()
}

fn read_connection<T>(
    path: &Path,
    read: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>,
) -> Option<T> {
    if !path.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(200));
    read(&conn).ok()
}

pub(crate) fn insert_messages(conn: &Connection, conv_id: &str, messages: &[StoredMessage]) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    for message in messages {
        let _ = conn.execute(
            "INSERT INTO messages
             (conversation_id, role, content, tool_name, tool_args, step_index, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                conv_id,
                message.role,
                message.content,
                message.tool_name,
                message.tool_args,
                message.step_index.map(|value| value as i64),
                now,
            ],
        );
    }
}
