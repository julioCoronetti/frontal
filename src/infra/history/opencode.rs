use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::protocol::TokenUsage;

pub fn default_path() -> Option<PathBuf> {
    crate::model_catalog::ModelCatalog::default_path()
}

pub fn load_usage(session_id: &str) -> Option<(TokenUsage, Option<String>)> {
    default_path().and_then(|path| load_usage_at(&path, session_id))
}

pub fn load_usage_at(path: &Path, session_id: &str) -> Option<(TokenUsage, Option<String>)> {
    if !path.is_file() || !session_id.starts_with("ses_") {
        return None;
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    connection.busy_timeout(Duration::from_millis(2_000)).ok()?;
    let payload: String = connection
        .query_row(
            "SELECT data FROM session_message
             WHERE session_id = ?1 AND type = 'assistant'
             ORDER BY seq DESC LIMIT 1",
            [session_id],
            |row| row.get(0),
        )
        .ok()?;
    let value: serde_json::Value = serde_json::from_str(&payload).ok()?;
    let tokens = value.get("tokens")?;
    let input = tokens
        .get("input")
        .and_then(|item| item.as_u64())
        .unwrap_or(0);
    let output = tokens
        .get("output")
        .and_then(|item| item.as_u64())
        .unwrap_or(0);
    let reasoning = tokens
        .get("reasoning")
        .and_then(|item| item.as_u64())
        .unwrap_or(0);
    let cache_read = tokens
        .get("cache")
        .and_then(|cache| cache.get("read"))
        .and_then(|item| item.as_u64())
        .unwrap_or(0);
    if input == 0 && output == 0 && reasoning == 0 && cache_read == 0 {
        return None;
    }
    let model = value.get("model").and_then(|model| {
        let provider = model.get("providerID").and_then(|item| item.as_str())?;
        let id = model.get("id").and_then(|item| item.as_str())?;
        Some(format!("{}/{}", provider, id))
    });
    Some((
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            thinking_tokens: reasoning,
            cache_read_tokens: cache_read,
            total_tokens: 0,
        },
        model,
    ))
}
