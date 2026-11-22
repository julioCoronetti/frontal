mod messages;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};
use serde::Deserialize;
use serde_json::Value;

use crate::config::BackendKind;
use crate::protocol::TokenUsage;

use super::{SessionRecord, TitleSource};

pub use messages::{load_opencode_messages, load_opencode_messages_at};

#[derive(Deserialize, Default)]
struct AssistantPayload {
    #[serde(default)]
    tokens: Option<MessageTokens>,
    #[serde(default)]
    model: Option<MessageModel>,
    #[serde(default)]
    time: MessageTime,
    #[serde(default)]
    content: Vec<Value>,
}

impl AssistantPayload {
    fn turn_duration_millis(&self) -> Option<u64> {
        self.time
            .completed
            .or(self.time.streamed)
            .zip(self.time.created)
            .map(|(end, start)| end.saturating_sub(start))
            .filter(|millis| *millis > 0)
    }
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
    #[serde(default)]
    variant: Option<String>,
}

impl MessageModel {
    fn full_id(&self) -> Option<String> {
        let provider = sanitize_part(self.provider_id.as_deref()?)?;
        let id = sanitize_part(self.id.as_deref()?)?;
        Some(format!("{}/{}", provider, id))
    }
}

pub fn default_path() -> Option<PathBuf> {
    crate::model_catalog::ModelCatalog::default_path()
}

pub fn normalize_timestamp_secs(raw: i64) -> i64 {
    if raw <= 0 {
        0
    } else if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    }
}

pub fn parse_opencode_model_json(raw: &str) -> (Option<String>, Option<String>) {
    let Ok(parsed) = serde_json::from_str::<MessageModel>(raw) else {
        return (None, None);
    };
    let model = parsed.full_id();
    let effort = parsed
        .variant
        .as_deref()
        .and_then(sanitize_part)
        .map(str::to_string);
    (model, effort)
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
    let connection = open_readonly(path)?;
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
    let tokens = payload.tokens.as_ref()?;
    let input = tokens.input;
    let output = tokens.output;
    let reasoning = tokens.reasoning;
    let cache_read = tokens.cache.read;
    if input == 0 && output == 0 && reasoning == 0 && cache_read == 0 {
        return None;
    }
    let duration_millis = payload.turn_duration_millis();
    let model = payload.model.and_then(|model| model.full_id());
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

pub fn load_opencode_conversations() -> Vec<SessionRecord> {
    default_path()
        .map(|path| load_opencode_conversations_at(&path))
        .unwrap_or_default()
}

pub fn load_opencode_conversations_at(path: &Path) -> Vec<SessionRecord> {
    let Some(conn) = open_readonly(path) else {
        return Vec::new();
    };
    let mut stmt = match conn.prepare(
        "SELECT id, title, model, COALESCE(time_updated, time_created, 0)
         FROM session_v2
         WHERE parent_id IS NULL
           AND (time_archived IS NULL OR time_archived = 0)
           AND id IS NOT NULL AND id != ''
         ORDER BY COALESCE(time_updated, time_created, 0) DESC
         LIMIT 200",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let mut records = Vec::new();
    if let Ok(mut rows) = stmt.query([]) {
        while let Ok(Some(row)) = rows.next() {
            let Ok(session_id) = row.get::<_, String>(0) else {
                continue;
            };
            let title: Option<String> = row.get(1).ok().flatten();
            let model_raw: Option<String> = row.get(2).ok().flatten();
            let raw_ts: i64 = row.get(3).unwrap_or(0);
            let final_title = clean_opencode_title(title);
            let (model, effort) = model_raw
                .as_deref()
                .map(parse_opencode_model_json)
                .unwrap_or((None, None));
            let title_source = if final_title.is_some() {
                TitleSource::Semantic
            } else {
                TitleSource::Default
            };
            records.push(SessionRecord {
                id: format!("ext-{}", session_id),
                conversation_id: Some(session_id),
                backend: Some(BackendKind::OpenCode),
                model,
                mode: None,
                effort,
                title: final_title,
                title_source,
                created_at: normalize_timestamp_secs(raw_ts),
            });
        }
    }
    records
}

fn query_session_col<T: rusqlite::types::FromSql>(
    path: &Path,
    session_id: &str,
    sql: &str,
) -> Option<T> {
    if !session_id.starts_with("ses_") {
        return None;
    }
    open_readonly(path)?
        .query_row(sql, params![session_id], |row| row.get(0))
        .ok()
}

pub fn query_opencode_title(session_id: &str) -> Option<String> {
    default_path().and_then(|path| query_opencode_title_at(&path, session_id))
}

pub fn query_opencode_title_at(path: &Path, session_id: &str) -> Option<String> {
    let title: Option<String> = query_session_col(
        path,
        session_id,
        "SELECT title FROM session_v2 WHERE id = ?1",
    )?;
    clean_opencode_title(title)
}

fn clean_opencode_title(raw: Option<String>) -> Option<String> {
    raw.as_deref()
        .map(str::trim)
        .filter(|t| {
            !t.is_empty()
                && !t.eq_ignore_ascii_case("new session")
                && !t.starts_with("New session - ")
                && !t.eq_ignore_ascii_case("untitled")
        })
        .map(str::to_string)
}

pub fn query_opencode_timestamp(session_id: &str) -> Option<i64> {
    default_path().and_then(|path| query_opencode_timestamp_at(&path, session_id))
}

pub fn query_opencode_timestamp_at(path: &Path, session_id: &str) -> Option<i64> {
    let raw_ts: i64 = query_session_col(
        path,
        session_id,
        "SELECT COALESCE(time_updated, time_created, 0) FROM session_v2 WHERE id = ?1",
    )?;
    let normalized = normalize_timestamp_secs(raw_ts);
    (normalized > 0).then_some(normalized)
}

pub fn query_opencode_model(session_id: &str) -> (Option<String>, Option<String>) {
    default_path()
        .map(|path| query_opencode_model_at(&path, session_id))
        .unwrap_or((None, None))
}

pub fn query_opencode_model_at(path: &Path, session_id: &str) -> (Option<String>, Option<String>) {
    let raw: Option<String> = query_session_col(
        path,
        session_id,
        "SELECT model FROM session_v2 WHERE id = ?1",
    )
    .flatten();
    raw.as_deref()
        .map(parse_opencode_model_json)
        .unwrap_or((None, None))
}

fn open_readonly(path: &Path) -> Option<Connection> {
    if !path.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(300));
    Some(conn)
}

fn sanitize_part(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        None
    } else {
        Some(trimmed)
    }
}
