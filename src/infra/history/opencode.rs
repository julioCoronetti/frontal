use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use crate::protocol::TokenUsage;

#[derive(Deserialize, Default)]
struct AssistantPayload {
    #[serde(default)]
    tokens: Option<MessageTokens>,
    #[serde(default)]
    model: Option<MessageModel>,
    #[serde(default)]
    time: MessageTime,
}

#[derive(Deserialize, Default)]
struct MessageTokens {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    reasoning: u64,
    #[serde(default)]
    cache: CacheTokens,
}

#[derive(Deserialize, Default)]
struct CacheTokens {
    #[serde(default)]
    read: u64,
}

#[derive(Deserialize, Default)]
struct MessageTime {
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    streamed: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
}

#[derive(Deserialize, Default)]
struct MessageModel {
    #[serde(default, rename = "providerID")]
    provider_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

pub fn default_path() -> Option<PathBuf> {
    crate::model_catalog::ModelCatalog::default_path()
}

pub fn load_usage(session_id: &str) -> Option<(TokenUsage, Option<String>, Option<u64>)> {
    default_path().and_then(|path| load_usage_at(&path, session_id))
}

pub fn load_usage_at(
    path: &Path,
    session_id: &str,
) -> Option<(TokenUsage, Option<String>, Option<u64>)> {
    if !path.is_file() || !session_id.starts_with("ses_") {
        return None;
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    connection.busy_timeout(Duration::from_millis(300)).ok()?;
    let payload: String = connection
        .query_row(
            "SELECT data FROM session_message
             WHERE session_id = ?1 AND type = 'assistant'
             ORDER BY seq DESC LIMIT 1",
            [session_id],
            |row| row.get(0),
        )
        .ok()?;
    let payload: AssistantPayload = serde_json::from_str(&payload).ok()?;
    let tokens = payload.tokens?;
    let input = tokens.input;
    let output = tokens.output;
    let reasoning = tokens.reasoning;
    let cache_read = tokens.cache.read;
    if input == 0 && output == 0 && reasoning == 0 && cache_read == 0 {
        return None;
    }
    let model = payload.model.and_then(|model| {
        let provider = sanitize_part(model.provider_id.as_deref()?)?;
        let id = sanitize_part(model.id.as_deref()?)?;
        Some(format!("{}/{}", provider, id))
    });
    let end = payload.time.completed.or(payload.time.streamed);
    let duration_millis = end
        .zip(payload.time.created)
        .map(|(end, start)| end.saturating_sub(start))
        .filter(|millis| *millis > 0);
    Some((
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            thinking_tokens: reasoning,
            cache_read_tokens: cache_read,
            total_tokens: 0,
        },
        model,
        duration_millis,
    ))
}

fn sanitize_part(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        None
    } else {
        Some(trimmed)
    }
}
